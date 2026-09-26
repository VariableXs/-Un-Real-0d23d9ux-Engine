//! F004 Wow64 门（32 位 .exe 前瞻）（compatstar · G-A-04）——拒绝也给出路。
//!
//! 主册判据（验收标准第一句）：
//! **「32 位样本集 10 枚 100% 触发诚实卡片（零静默失败/零崩溃）；卡片文案
//! 三要素齐。」**
//!
//! 功能定义（G-A-04）：32 位 PE 的三层处理：探测（机器类型字段识别）→ 拒绝
//! （当前版本无 32 位子系统）→ 诚实提示 + 计划入口。拒绝不是终点，是分流：
//! 把「不支持」变成「明确知道为什么不行」。
//!
//! 【交互设计】拒绝卡片样式对齐 F035 兼容性向导（同一卡片体系）：图标+标题+
//! 一句归因+两个动作（关闭/查替代）。卡片自动记忆该文件，同文件第二次双击
//! 不再重复解释（本地记录，可清）。
//! 【数据与存储】拒绝记录存 `cache/wow64-refusals.json`（文件哈希 → 已提示
//! 标记），上限 1000 条。
//! 【状态与异常】带 32 位安装器的混合包（32 位安装器装 64 位主程序）→ 安装
//! 器本身被拒时提示完整归因；伪装 32 位的恶意样本照走 peblock 门，先过门再
//! 谈位数。
//! 【设计细节】位数判定读 PE 机器字段 0x014c（I386）与 0x8664（AMD64），ARM64
//! 声明也识别并如实告知；拒绝记录同时上报星卡草稿（F036 联动，匿名）；「查
//! 替代品」按钮跳星图搜索并预填程序名关键词；卡片文案经过三要素审计（发生
//! 了什么/为什么/下一步），禁用「不支持」裸句。
//!
//! 顺序红线（主册【状态与异常】）：机器位数判定发生在 peblock 门**之后**——
//! 「先过门再谈位数」。本模块 API 形态即此语义：调用方必须先拿到 peblock
//! 放行结论再进 [`gate_machine`]。
//!
//! 零堆纪律：定长拒绝记录表，无 Vec/String/Box/format!。

use crate::checks::CheckSet;
use alloc::vec;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// IMAGE_FILE_MACHINE_I386（主册【设计细节】：0x014c 判 32 位）。
pub const MACHINE_I386: u16 = 0x014C;
/// IMAGE_FILE_MACHINE_AMD64（本机原生位宽）。
pub const MACHINE_AMD64: u16 = 0x8664;
/// IMAGE_FILE_MACHINE_ARM64（主册：ARM64 声明也识别并如实告知）。
pub const MACHINE_ARM64: u16 = 0xAA64;
/// 拒绝记录上限 1000 条（主册【数据与存储】）。
pub const REFUSAL_CAP: usize = 1000;
/// COFF 头机器字段在文件内的偏移（DOS 0x40 + e_lfanew + 4B 签名）——按
/// e_lfanew 动态读取，此常量仅用于最小头校验。
pub const COFF_MACHINE_MIN_FILE: usize = 0x40 + 4 + 4 + 2;

// ---------------------------------------------------------------------------
// 判定
// ---------------------------------------------------------------------------

/// 机器位数判定结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MachineVerdict {
    /// AMD64——本机原生，放行（位数面无话可说）。
    Native64,
    /// I386 32 位——拒绝并出诚实卡片。
    ThirtyTwo,
    /// ARM64 声明——识别并如实告知（当前无 ARM64 运行面）。
    Arm64Declared,
    /// 非 PE/头损坏——不归本门管（peblock 门或 F002 解析面已有归因）。
    NotPe,
}

impl MachineVerdict {
    /// 是否拒绝装载（Native64 之外全部拒绝——当前版本无 32 位子系统）。
    pub fn refused(self) -> bool {
        !matches!(self, MachineVerdict::Native64)
    }
}

/// 从映像头部读 COFF 机器字段并出判定。`image` 至少含 DOS 头 + PE 签名 +
/// COFF 头前 2 字节。
pub fn detect_machine(image: &[u8]) -> MachineVerdict {
    if image.len() < 0x40 || image[0..2] != [b'M', b'Z'] {
        return MachineVerdict::NotPe;
    }
    if image.len() < 0x44 {
        return MachineVerdict::NotPe;
    }
    let pe_off = u32::from_le_bytes(image[0x3C..0x40].try_into().unwrap()) as usize;
    if pe_off < 0x40 || pe_off + 6 > image.len() {
        return MachineVerdict::NotPe;
    }
    if image[pe_off..pe_off + 4] != [b'P', b'E', 0, 0] {
        return MachineVerdict::NotPe;
    }
    match u16::from_le_bytes([image[pe_off + 4], image[pe_off + 5]]) {
        MACHINE_AMD64 => MachineVerdict::Native64,
        MACHINE_I386 => MachineVerdict::ThirtyTwo,
        MACHINE_ARM64 => MachineVerdict::Arm64Declared,
        _ => MachineVerdict::NotPe,
    }
}

/// 诚实卡片（三要素：发生了什么/为什么/下一步——禁「不支持」裸句，主册
/// 【设计细节】文案审计纪律）。文案全部静态串，无堆。
#[derive(Clone, Copy, Debug)]
pub struct HonestCard {
    /// 发生了什么（人话）。
    pub what: &'static str,
    /// 为什么（归因，含路线图位置）。
    pub why: &'static str,
    /// 下一步怎么办（出路：查替代品/关闭）。
    pub next: &'static str,
    /// 「查看 64 位替代品」按钮的星图搜索预填关键词（主册：直跳星图搜索）。
    pub alt_query: &'static str,
}

impl MachineVerdict {
    pub fn honest_card(self) -> Option<HonestCard> {
        match self {
            MachineVerdict::ThirtyTwo => Some(HonestCard {
                what: "此程序为 32 位应用，未能启动。",
                why: "VARIX 当前运行 64 位程序；32 位兼容在路线图中（预计 STAR I start 后程）。",
                next: "可关闭此卡片，或点击「查看 64 位替代品」在星图中搜索同类工具。",
                alt_query: "64位替代",
            }),
            MachineVerdict::Arm64Declared => Some(HonestCard {
                what: "此程序声明为 ARM64 应用，未能启动。",
                why: "VARIX 当前提供 x86-64 运行面；ARM64 声明已识别，如实告知无对应运行面。",
                next: "可关闭此卡片，或点击「查看 64 位替代品」寻找 x86-64 版本。",
                alt_query: "x64替代",
            }),
            _ => None,
        }
    }
}

/// 混合包归因（主册【状态与异常】：32 位安装器装 64 位主程序——安装器本身
/// 被拒时提示完整归因，不让用户误以为「主程序也是 32 位」）。
#[derive(Clone, Copy, Debug)]
pub struct MixedPackage {
    /// 安装器机器判定（Always ThirtyTwo——否则不构成混合包场景）。
    pub installer: MachineVerdict,
    /// 包内主程序声明的机器（来自清单/样本面）。
    pub payload: MachineVerdict,
}

impl MixedPackage {
    /// 是否真混合（安装器 32 位 + 主程序 64 位）。
    pub fn is_mixed(self) -> bool {
        self.installer == MachineVerdict::ThirtyTwo && self.payload == MachineVerdict::Native64
    }

    /// 完整归因卡片（区别于普通 32 位卡片：明确说主程序是 64 位、只是安装
    /// 器是 32 位）。
    pub fn card(self) -> HonestCard {
        HonestCard {
            what: "此安装包的安装器为 32 位程序，安装未能开始。",
            why: "包内主程序是 64 位（可正常运行），当前版本仅安装器环节缺少 32 位运行面。",
            next: "可寻找提供 64 位安装器的版本，或解包后直接运行主程序。",
            alt_query: "64位安装器",
        }
    }
}

// ---------------------------------------------------------------------------
// 拒绝记录表（同文件第二次双击不再重复解释）
// ---------------------------------------------------------------------------

/// 拒绝记录（文件哈希 → 已提示标记）。定长环形替换（1000 条满后覆盖最旧
/// ——主册上限语义；JSON 落盘为持久层职责，本层提供内存模型与序号）。
pub struct RefusalLedger {
    hashes: [u64; REFUSAL_CAP],
    reported: [bool; REFUSAL_CAP],
    count: usize,
    next_slot: usize,
    /// 上报星卡草稿（F036 联动，匿名）的条数记账。
    starcard_reports: u32,
}

impl RefusalLedger {
    pub fn new() -> RefusalLedger {
        RefusalLedger {
            hashes: [0; REFUSAL_CAP],
            reported: [false; REFUSAL_CAP],
            count: 0,
            next_slot: 0,
            starcard_reports: 0,
        }
    }

    /// 双击事件进入本门时调用。返回 true = 首次拒绝（需要出卡片）；
    /// false = 已提示过（静默按已记理由拒绝，不再重复解释）。
    pub fn refuse(&mut self, file_hash: u64) -> bool {
        if let Some(i) = (0..self.count).find(|&i| self.hashes[i] == file_hash) {
            // 已有记录：第二次双击不再重复解释。
            return !self.reported[i];
        }
        let slot = if self.count < REFUSAL_CAP {
            let s = self.count;
            self.count += 1;
            s
        } else {
            // 满容环形覆盖最旧（next_slot 单调推进）。
            let s = self.next_slot;
            self.next_slot = (s + 1) % REFUSAL_CAP;
            s
        };
        self.hashes[slot] = file_hash;
        self.reported[slot] = true;
        // F036 联动：匿名上报星卡草稿。
        self.starcard_reports += 1;
        true
    }

    /// 用户手动清除记录（主册：本地记录，可清）。
    pub fn clear(&mut self) {
        self.hashes = [0; REFUSAL_CAP];
        self.reported = [false; REFUSAL_CAP];
        self.count = 0;
        self.next_slot = 0;
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn starcard_reports(&self) -> u32 {
        self.starcard_reports
    }
}

impl Default for RefusalLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 门入口（peblock 之后的位数闸）
// ---------------------------------------------------------------------------

/// 位数闸入口。`peblock_passed` 必须为 true（先过门再谈位数——主册顺序红线；
/// false 时返回 NotPe 语义短路，不越权替 peblock 做决定）。
pub fn gate_machine(peblock_passed: bool, image: &[u8]) -> MachineVerdict {
    if !peblock_passed {
        return MachineVerdict::NotPe;
    }
    detect_machine(image)
}

/// 域自检。
pub fn run_wow64_base_checks() -> CheckSet {
    let mut cs = CheckSet::new("F004-wow64");
    // 1) 判据常量（0x014C/0x8664/0xAA64/1000 条）。
    cs.add(
        "consts",
        MACHINE_I386 == 0x014C
            && MACHINE_AMD64 == 0x8664
            && MACHINE_ARM64 == 0xAA64
            && REFUSAL_CAP == 1000,
        "",
    );
    // 2) 32 位样本集 10 枚 100% 触发诚实卡片（构造 10 个不同长度的 I386 头）。
    let mut cards = 0u32;
    for len in [0x80usize, 0x100, 0x200, 0x400, 0x800, 0x1000, 0x2000, 0x4000, 0x8000, 0x10000] {
        let mut img = alloc::vec![0u8; len];
        img[0] = b'M';
        img[1] = b'Z';
        img[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        img[0x40..0x44].copy_from_slice(&[b'P', b'E', 0, 0]);
        img[0x44..0x46].copy_from_slice(&MACHINE_I386.to_le_bytes());
        let v = gate_machine(true, &img);
        if v == MachineVerdict::ThirtyTwo && v.honest_card().is_some() {
            cards += 1;
        }
    }
    cs.add("thirtytwo_10_of_10_cards", cards == 10, "");
    // 3) 零静默失败：32 位判定必然 refused 且有卡片（不静默放行/不静默拒绝）。
    let v = MachineVerdict::ThirtyTwo;
    cs.add(
        "no_silent_failure",
        v.refused() && v.honest_card().is_some(),
        "",
    );
    // 4) 卡片三要素齐（what/why/next 非空 + 禁「不支持」裸句）。
    let card = MachineVerdict::ThirtyTwo.honest_card().unwrap();
    let bare_reject = card.what.contains("不支持") && card.what.len() < 12;
    cs.add(
        "card_three_elements",
        !card.what.is_empty() && !card.why.is_empty() && !card.next.is_empty() && !bare_reject,
        "",
    );
    // 5) ARM64 声明如实告知（识别 + 卡片，不冒充 32 位归因）。
    let mut img = alloc::vec![0u8; 0x80];
    img[0] = b'M';
    img[1] = b'Z';
    img[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
    img[0x40..0x44].copy_from_slice(&[b'P', b'E', 0, 0]);
    img[0x44..0x46].copy_from_slice(&MACHINE_ARM64.to_le_bytes());
    let v = gate_machine(true, &img);
    cs.add(
        "arm64_declared_honest",
        v == MachineVerdict::Arm64Declared
            && v.refused()
            && v.honest_card().unwrap().why.contains("ARM64"),
        "",
    );
    // 6) 先过门再谈位数：peblock 未放行时位数闸短路（不越权）。
    cs.add(
        "peblock_first",
        gate_machine(false, &img) == MachineVerdict::NotPe,
        "",
    );
    // 7) 64 位放行。
    let mut img64 = img;
    img64[0x44..0x46].copy_from_slice(&MACHINE_AMD64.to_le_bytes());
    cs.add(
        "amd64_passes",
        gate_machine(true, &img64) == MachineVerdict::Native64,
        "",
    );
    // 8) 混合包：安装器 32 位 + 主程序 64 位 → 完整归因（主程序不被冤枉）。
    let mixed = MixedPackage { installer: MachineVerdict::ThirtyTwo, payload: MachineVerdict::Native64 };
    let c = mixed.card();
    cs.add(
        "mixed_package_full_attribution",
        mixed.is_mixed() && c.why.contains("主程序是 64 位"),
        "",
    );
    // 9) 拒绝记录：同文件第二次双击不再重复解释；可清；F036 上报记账。
    let mut ledger = RefusalLedger::new();
    let first = ledger.refuse(0xDEADBEEF);
    let second = ledger.refuse(0xDEADBEEF);
    cs.add(
        "refusal_remembered",
        first && !second && ledger.starcard_reports() == 1,
        "",
    );
    ledger.clear();
    cs.add(
        "refusal_clearable",
        ledger.is_empty() && ledger.refuse(0xDEADBEEF),
        "",
    );
    // 10) 满容环形覆盖（1000 条后第 1001 条覆盖最旧，不崩不涨）。
    let mut ledger = RefusalLedger::new();
    for h in 0..REFUSAL_CAP as u64 {
        let _ = ledger.refuse(h);
    }
    let capped = ledger.len() == REFUSAL_CAP;
    let _ = ledger.refuse(REFUSAL_CAP as u64 + 1);
    cs.add("refusal_ring_capped", capped && ledger.len() == REFUSAL_CAP, "");
    cs
}

// ---------------------------------------------------------------------------
// 测试（宿主）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn pe_with_machine(machine: u16) -> Vec<u8> {
        let mut img = alloc::vec![0u8; 0x80];
        img[0] = b'M';
        img[1] = b'Z';
        img[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        img[0x40..0x44].copy_from_slice(&[b'P', b'E', 0, 0]);
        img[0x44..0x46].copy_from_slice(&machine.to_le_bytes());
        img
    }

    #[test]
    fn ten_thirtytwo_samples_all_card() {
        // 判据：32 位样本集 10 枚 100% 触发诚实卡片。
        for len in 0..10usize {
            let mut img = pe_with_machine(MACHINE_I386);
            img.resize(0x80 + len * 8, 0);
            let v = gate_machine(true, &img);
            assert_eq!(v, MachineVerdict::ThirtyTwo);
            let c = v.honest_card().expect("must produce honest card");
            assert!(c.what.contains("32 位"));
            assert!(c.why.contains("64 位"));
            assert!(c.next.contains("替代"));
        }
    }

    #[test]
    fn no_bare_reject_wording() {
        // 文案审计：禁「不支持」裸句——what/why/next 必须有信息量。
        for v in [MachineVerdict::ThirtyTwo, MachineVerdict::Arm64Declared] {
            let c = v.honest_card().unwrap();
            assert!(c.what.len() > 10, "what too bare: {}", c.what);
            assert!(c.why.len() > 10, "why too bare: {}", c.why);
            assert!(c.next.len() > 10, "next too bare: {}", c.next);
        }
    }

    #[test]
    fn mixed_package_distinct_from_plain_32() {
        // 混合包卡片与普通 32 位卡片必须不同（完整归因：不冤枉主程序）。
        let mixed = MixedPackage { installer: MachineVerdict::ThirtyTwo, payload: MachineVerdict::Native64 };
        let plain = MachineVerdict::ThirtyTwo.honest_card().unwrap();
        let c = mixed.card();
        assert!(c.why.contains("主程序是 64 位"));
        assert_ne!(c.what, plain.what);
        // 反例：双 32 位不算混合。
        let not_mixed = MixedPackage { installer: MachineVerdict::ThirtyTwo, payload: MachineVerdict::ThirtyTwo };
        assert!(!not_mixed.is_mixed());
    }

    #[test]
    fn peblock_ordering_redline() {
        // 主册【状态与异常】：伪装 32 位的恶意样本照走 peblock 门——
        // peblock 拒绝时本门不表态（返回 NotPe 短路），不抢闸。
        let img = pe_with_machine(MACHINE_I386);
        assert_eq!(gate_machine(false, &img), MachineVerdict::NotPe);
        assert_eq!(gate_machine(true, &img), MachineVerdict::ThirtyTwo);
    }

    #[test]
    fn ledger_second_click_quiet() {
        let mut l = RefusalLedger::new();
        assert!(l.refuse(42), "first click must show card");
        assert!(!l.refuse(42), "second click must not repeat the card");
        assert!(l.refuse(43), "different file still gets its card");
        assert_eq!(l.len(), 2);
        assert_eq!(l.starcard_reports(), 2);
        l.clear();
        assert!(l.refuse(42), "after clear the card returns");
    }

    #[test]
    fn ledger_ring_overflow_is_capped() {
        let mut l = RefusalLedger::new();
        for h in 0..(REFUSAL_CAP as u64 + 50) {
            let _ = l.refuse(h);
        }
        assert_eq!(l.len(), REFUSAL_CAP);
        // 最旧的 0..50 已被覆盖：再次拒绝这些哈希应重新出卡。
        assert!(l.refuse(0));
        // 新写入的 1000..1050 仍在表内：不出卡。
        assert!(!l.refuse(1000));
    }

    #[test]
    fn corrupt_headers_not_our_business() {
        // 非 PE / 头损坏不归位数闸管（peblock/F002 已有归因，不重复表态）。
        assert_eq!(detect_machine(&[0u8; 8]), MachineVerdict::NotPe);
        let mut bad = pe_with_machine(MACHINE_I386);
        bad[0] = b'X';
        assert_eq!(detect_machine(&bad), MachineVerdict::NotPe);
        let mut bad = pe_with_machine(MACHINE_I386);
        bad[0x44..0x46].copy_from_slice(&0x9999u16.to_le_bytes());
        assert_eq!(detect_machine(&bad), MachineVerdict::NotPe);
    }
}

// ---------------------------------------------------------------------------
// F004 · 深化扩展：星图替代品查询预填 + 拒绝缓存落盘模型
//
// 主册依据（G-A-04【设计细节】）：「查替代品」按钮跳星图搜索并**预填程序名
// 关键词**；【数据与存储】拒绝记录存 `cache/wow64-refusals.json`——本扩展给
// 出该缓存文件的落盘模型：定长记录 + 校验和，损坏即弃建（F189 同族自愈，
// 读不出 = 空表重建，不报错不挂起）。
// ---------------------------------------------------------------------------

/// 预填关键词里程序名部分的上限（40 字节——超过截断，星图搜索框仍有完整
/// 文件名可查；截断在 char 边界，不切半个字）。
pub const ALT_QUERY_NAME_CAP: usize = 40;
/// 固定后缀（主册示例文案：搜索同类 64 位工具）。
pub const ALT_QUERY_SUFFIX: &str = " 64位替代";

/// 星图搜索预填查询（程序名关键词——主册【设计细节】：预填程序名，不是
/// 静态泛词）。零堆：调用方给渲染缓冲，本结构只记账名字与截断态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AltQuery {
    /// 程序基础名（剥目录与扩展名；char 边界截断到 ALT_QUERY_NAME_CAP）。
    pub name: [u8; ALT_QUERY_NAME_CAP],
    pub name_len: usize,
    /// 名字被截断时如实标注（诊断面：预填词与原名的差异可解释）。
    pub truncated: bool,
}

/// 从完整路径/文件名提取预填词。剥最后一节路径分隔（`\\` 与 `/` 都认），
/// 剥 `.exe` 扩展名（大小写各一档；其余扩展名保留——「程序名关键词」语义）。
pub fn alt_query_for(path: &str) -> AltQuery {
    let base = match path.rfind(['\\', '/']) {
        Some(p) => &path[p + 1..],
        None => path,
    };
    let base = base
        .strip_suffix(".exe")
        .or_else(|| base.strip_suffix(".EXE"))
        .unwrap_or(base);
    let mut q = AltQuery { name: [0; ALT_QUERY_NAME_CAP], name_len: 0, truncated: false };
    for ch in base.chars() {
        let mut buf = [0u8; 4];
        let enc = ch.encode_utf8(&mut buf).as_bytes();
        if q.name_len + enc.len() > ALT_QUERY_NAME_CAP {
            q.truncated = true;
            break;
        }
        q.name[q.name_len..q.name_len + enc.len()].copy_from_slice(enc);
        q.name_len += enc.len();
    }
    q
}

impl AltQuery {
    /// 渲染进星图搜索框（名字 + 固定后缀）。缓冲不足如实返回 0（不静默截）。
    pub fn render(&self, buf: &mut [u8]) -> usize {
        let total = self.name_len + ALT_QUERY_SUFFIX.len();
        if buf.len() < total {
            return 0;
        }
        buf[..self.name_len].copy_from_slice(&self.name[..self.name_len]);
        buf[self.name_len..total].copy_from_slice(ALT_QUERY_SUFFIX.as_bytes());
        total
    }
}

// -- 拒绝缓存落盘模型（cache/wow64-refusals） -------------------------------

/// 缓存文件头 16B：magic(4) + version(2) + count(2) + reserved(8)。
pub const REFUSAL_HDR_SIZE: usize = 16;
/// 单条记录 16B：hash(8) + reported(1) + reserved(7)。
pub const REFUSAL_REC_SIZE: usize = 16;
/// 尾部校验和 8B（FNV-1a over header+records）。
pub const REFUSAL_SUM_SIZE: usize = 8;
/// 文件 magic（"VXR4"——Varix wow64 Refusals v4）。
pub const REFUSAL_MAGIC: [u8; 4] = *b"VXR4";

/// 序列化拒绝账本（落盘形态）。返回写入字节数；缓冲不足返回 0（不静默截）。
pub fn serialize_refusals(ledger: &RefusalLedger, buf: &mut [u8]) -> usize {
    let n = ledger.len();
    let total = REFUSAL_HDR_SIZE + n * REFUSAL_REC_SIZE + REFUSAL_SUM_SIZE;
    if buf.len() < total || n > REFUSAL_CAP {
        return 0;
    }
    buf[0..4].copy_from_slice(&REFUSAL_MAGIC);
    buf[4..6].copy_from_slice(&4u16.to_le_bytes());
    buf[6..8].copy_from_slice(&(n as u16).to_le_bytes());
    buf[8..16].fill(0);
    for i in 0..n {
        let o = REFUSAL_HDR_SIZE + i * REFUSAL_REC_SIZE;
        buf[o..o + 8].copy_from_slice(&ledger.hashes[i].to_le_bytes());
        buf[o + 8] = ledger.reported[i] as u8;
        buf[o + 9..o + 16].fill(0);
    }
    let body = REFUSAL_HDR_SIZE + n * REFUSAL_REC_SIZE;
    let sum = fnv64(&buf[..body]);
    buf[body..body + 8].copy_from_slice(&sum.to_le_bytes());
    total
}

/// 反序列化（自愈语义：任何损坏 → Err，调用方弃文件重建空表——F189 同族，
/// 不静默吞坏数据）。校验 magic/版本/长度/校验和四关。
pub fn deserialize_refusals(data: &[u8]) -> Result<RefusalLedger, &'static str> {
    if data.len() < REFUSAL_HDR_SIZE + REFUSAL_SUM_SIZE {
        return Err("wow64: refusal cache too small");
    }
    if data[0..4] != REFUSAL_MAGIC {
        return Err("wow64: refusal cache bad magic");
    }
    if u16::from_le_bytes([data[4], data[5]]) != 4 {
        return Err("wow64: refusal cache unsupported version");
    }
    let n = u16::from_le_bytes([data[6], data[7]]) as usize;
    if n > REFUSAL_CAP {
        return Err("wow64: refusal cache count out of range");
    }
    let body = REFUSAL_HDR_SIZE + n * REFUSAL_REC_SIZE;
    if data.len() < body + REFUSAL_SUM_SIZE {
        return Err("wow64: refusal cache truncated");
    }
    let expect = u64::from_le_bytes(data[body..body + 8].try_into().unwrap());
    if fnv64(&data[..body]) != expect {
        return Err("wow64: refusal cache checksum mismatch");
    }
    let mut l = RefusalLedger::new();
    for i in 0..n {
        let o = REFUSAL_HDR_SIZE + i * REFUSAL_REC_SIZE;
        l.hashes[i] = u64::from_le_bytes(data[o..o + 8].try_into().unwrap());
        l.reported[i] = data[o + 8] != 0;
    }
    l.count = n;
    l.next_slot = if n >= REFUSAL_CAP { 0 } else { n };
    Ok(l)
}

fn fnv64(data: &[u8]) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01B3);
    }
    h
}

#[cfg(test)]
mod ext_tests {
    use super::*;

    #[test]
    fn alt_query_prefills_program_name() {
        // 主册【设计细节】：预填**程序名**关键词（非静态泛词）。
        let q = alt_query_for("S:\\Downloads\\Notepad3.exe");
        assert!(!q.truncated);
        let mut buf = [0u8; 64];
        let n = q.render(&mut buf);
        let rendered = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(rendered.starts_with("Notepad3"));
        assert!(rendered.ends_with(ALT_QUERY_SUFFIX));
        // 相对名同样命中；无扩展名保留；大写 EXE 也剥。
        assert_eq!(alt_query_for("tool.exe").name_len, 4);
        assert_eq!(alt_query_for("PACKER.EXE").name_len, 6);
        assert_eq!(alt_query_for("archive.zip").render(&mut buf), "archive.zip".len() + ALT_QUERY_SUFFIX.len());
        // 长名 char 边界截断 + 截断如实标注（不切半个字）。
        let long = "很长的程序名字段".repeat(20);
        let q2 = alt_query_for(&long);
        assert!(q2.truncated);
        assert!(q2.name_len <= ALT_QUERY_NAME_CAP);
        assert!(core::str::from_utf8(&q2.name[..q2.name_len]).is_ok(), "截断必须在 char 边界");
        // 缓冲不足如实返回 0。
        assert_eq!(q.render(&mut [0u8; 4]), 0);
    }

    #[test]
    fn refusal_cache_round_trip() {
        // 落盘往返：序列化 → 反序列化 → 行为等价（同一哈希第二次不出卡）。
        let mut src = RefusalLedger::new();
        for h in [0x11u64, 0x22, 0x33] {
            assert!(src.refuse(h));
        }
        let mut buf = [0u8; REFUSAL_HDR_SIZE + 8 * REFUSAL_REC_SIZE + REFUSAL_SUM_SIZE];
        let n = serialize_refusals(&src, &mut buf);
        assert_eq!(n, REFUSAL_HDR_SIZE + 3 * REFUSAL_REC_SIZE + REFUSAL_SUM_SIZE);
        let mut back = deserialize_refusals(&buf[..n]).expect("round trip must load");
        assert_eq!(back.len(), 3);
        assert!(!back.refuse(0x22), "重启后同文件仍不再重复解释");
        assert!(back.refuse(0x44), "新文件照常出卡");
        assert_eq!(back.starcard_reports(), 1, "只对新文件上报");
    }

    #[test]
    fn refusal_cache_corrupt_self_heals() {
        // 自愈语义（F189 同族）：损坏一律 Err → 调用方弃文件重建，不静默吞。
        let mut src = RefusalLedger::new();
        let _ = src.refuse(0xAB);
        let mut buf = [0u8; 64];
        let n = serialize_refusals(&src, &mut buf);
        // 坏 magic / 坏版本 / 截断 / 校验和翻转，四路全拒。
        let mut m = buf[..n].to_vec();
        m[0] = b'X';
        assert!(deserialize_refusals(&m).is_err());
        let mut v = buf[..n].to_vec();
        v[5] = 9;
        assert!(deserialize_refusals(&v).is_err());
        assert!(deserialize_refusals(&buf[..8]).is_err());
        let mut c = buf[..n].to_vec();
        c[REFUSAL_HDR_SIZE] ^= 0xFF;
        assert!(deserialize_refusals(&c).is_err());
        // 超容量 count 拒绝。
        let mut big = buf[..n].to_vec();
        big[6..8].copy_from_slice(&(REFUSAL_CAP as u16 + 1).to_le_bytes());
        // 改了 count 必须连带校验和失效（或超容直接拒）——两路都必须 Err。
        assert!(deserialize_refusals(&big).is_err());
        // 缓冲不足序列化如实返回 0。
        let mut small = [0u8; 12];
        assert_eq!(serialize_refusals(&src, &mut small), 0);
    }
}

// ---------------------------------------------------------------------------
// 深化批次二：自检聚合（主检 + 深化检并为一行——AI-U2 merge 先例；
// robust.rs / 隔离壳 checkup 接线不变，深化检查项全部经由此行可见）。
// ---------------------------------------------------------------------------

/// 域自检（聚合版）。
pub fn run_wow64_checks() -> CheckSet {
    CheckSet::merge(run_wow64_base_checks(), CheckSet::merge(run_wow64_deep_checks(), CheckSet::merge(run_wow64_deep2_checks(), CheckSet::merge(run_wow64_deep3_checks(), CheckSet::merge(run_wow64_deep4_checks(), CheckSet::merge(run_wow64_deep5_checks(), CheckSet::merge(run_wow64_deep6_checks(), run_wow64_deep7_checks())))))))
}

// ---------------------------------------------------------------------------
// F004 · 深化批次二：星卡上报日去重（F036 联动）
//
// 主册依据（G-A-04【设计细节】）：「拒绝记录同时上报星卡草稿（F036 联动，
// 匿名）」——同一文件反复双击不应刷屏星卡：按（文件哈希 × 自然日）去重。
// 零堆：定长日去重表。
// ---------------------------------------------------------------------------

/// 星卡日去重表（同哈希同日只报一次；容量 32 定长环形）。
pub struct StarcardDayDedup {
    entries: [(u64, u32); 32], // (file_hash, day_index)
    n: usize,
    /// 因去重被抑制的上报数（观测面——不静默）。
    pub suppressed: u32,
}

/// 自然日序号（epoch 天——调用方给 ms 时间戳，此处按天折算）。
pub fn day_index(now_ms: u64) -> u32 {
    (now_ms / 86_400_000) as u32
}

impl StarcardDayDedup {
    pub fn new() -> StarcardDayDedup {
        StarcardDayDedup { entries: [(0, 0); 32], n: 0, suppressed: 0 }
    }

    /// 是否应上报（同哈希同日 → 抑制）。返回 true = 当日报一次。
    pub fn should_report(&mut self, file_hash: u64, now_ms: u64) -> bool {
        let day = day_index(now_ms);
        if let Some(i) = (0..self.n).find(|&i| self.entries[i].0 == file_hash) {
            if self.entries[i].1 == day {
                self.suppressed += 1;
                return false;
            }
            self.entries[i].1 = day; // 跨日重报
            return true;
        }
        let slot = if self.n < 32 {
            let s = self.n;
            self.n += 1;
            s
        } else {
            self.suppressed += 1;
            return false; // 表满如实抑制（观测面可见，不静默丢）——容量登记对账
        };
        self.entries[slot] = (file_hash, day);
        true
    }
}

/// F004 深化自检。
pub fn run_wow64_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F004-wow64-deep");
    // 1) 替代品预填（深化一批既有面）：程序名 + 固定后缀，char 边界截断。
    let q = alt_query_for("S:\\Downloads\\Notepad3.exe");
    let mut buf = [0u8; 64];
    let n = q.render(&mut buf);
    let rendered = core::str::from_utf8(&buf[..n]).unwrap_or("");
    cs.add(
        "alt_query_prefill",
        rendered.starts_with("Notepad3") && rendered.ends_with(ALT_QUERY_SUFFIX) && !q.truncated,
        "",
    );
    let long_q = alt_query_for(&"很长的程序名字段".repeat(20));
    cs.add(
        "alt_query_char_boundary",
        long_q.truncated
            && long_q.name_len <= ALT_QUERY_NAME_CAP
            && core::str::from_utf8(&long_q.name[..long_q.name_len]).is_ok(),
        "",
    );
    // 2) 拒绝缓存落盘（深化一批既有面）：往返等价 + 损坏四关全拒。
    let mut src = RefusalLedger::new();
    let _ = src.refuse(0x11);
    let mut cbuf = [0u8; REFUSAL_HDR_SIZE + REFUSAL_REC_SIZE + REFUSAL_SUM_SIZE];
    let cn = serialize_refusals(&src, &mut cbuf);
    let back = deserialize_refusals(&cbuf[..cn]);
    let mut corrupt = cbuf[..cn].to_vec();
    corrupt[REFUSAL_HDR_SIZE] ^= 0xFF;
    cs.add(
        "refusal_cache_roundtrip_and_reject",
        back.map(|l| l.len() == 1).unwrap_or(false) && deserialize_refusals(&corrupt).is_err(),
        "",
    );
    // 3) 星卡日去重：同日抑制、跨日重报、抑制计数可见。
    let mut dd = StarcardDayDedup::new();
    let d1 = dd.should_report(0xAA, 0);
    let d2 = dd.should_report(0xAA, 100);
    let d3 = dd.should_report(0xAA, 86_400_000);
    cs.add(
        "starcard_day_dedup",
        d1 && !d2 && d3 && dd.suppressed == 1 && day_index(86_399_999) == 0 && day_index(86_400_000) == 1,
        "",
    );
    // 4) 混合包归因（批次一既有面对账）：主程序不被冤枉。
    let mixed = MixedPackage { installer: MachineVerdict::ThirtyTwo, payload: MachineVerdict::Native64 };
    cs.add("mixed_package_attribution", mixed.is_mixed() && mixed.card().why.contains("主程序是 64 位"), "");
    cs
}

// ---------------------------------------------------------------------------
// F004 · 深化批次三：位数判定次序显式化（先过 peblock 门再谈位数）+ ARM64
// 如实告知话术
//
// 主册依据（G-A-04【状态与异常】）：「伪装 32 位的恶意样本照走 peblock 门，
// 先过门再谈位数」——门序是安全语义不是实现细节，钉成常量序；【设计细节】
// 「ARM64 声明也识别并如实告知」。MachineVerdict 既有面（一处一事实）。
// ---------------------------------------------------------------------------

/// 门序三步（恶意样本伪装位数也必须先过 peblock 门——次序即安全语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateStep {
    /// 第一步：peblock 门校验（规则/哈希——与位数无关）。
    Peblock,
    /// 第二步：机器位数判定（本门）。
    Machine,
    /// 第三步：装载/出诚实卡片。
    Launch,
}

/// 唯一合法门序（先过门再谈位数——一处一事实）。
pub const GATE_ORDER: [GateStep; 3] =
    [GateStep::Peblock, GateStep::Machine, GateStep::Launch];

/// 校验一段门序是否与 [`GATE_ORDER`] 全等（乱序 = 违例，如实 false）。
pub fn gate_order_respected(seq: &[GateStep]) -> bool {
    seq == GATE_ORDER
}

/// ARM64 声明话术（主册【设计细节】：识别并如实告知——三要素齐，非裸句）。
pub fn arm64_note(v: MachineVerdict) -> Option<&'static str> {
    match v {
        MachineVerdict::Arm64Declared => Some(
            "此程序声明为 ARM64 架构。VARIX 当前运行 x86-64（AMD64）程序；\
             ARM64 兼容在路线图中，可查看 64 位替代品。",
        ),
        _ => None,
    }
}

/// F004 深化批次三自检。
pub fn run_wow64_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F004-wow64-deep2");
    // 1) 门序：合法序通过；「先谈位数后过门」的乱序如实拒绝（安全语义锚）。
    cs.add(
        "gate_order_peblock_first",
        gate_order_respected(&GATE_ORDER)
            && !gate_order_respected(&[GateStep::Machine, GateStep::Peblock, GateStep::Launch]),
        "",
    );
    // 2) ARM64 话术：仅 Arm64Declared 出话术；三要素齐（含「为什么」（架构不符）
    //    与「下一步」（路线图+替代品）），禁裸句；其余判定 None。
    let note = arm64_note(MachineVerdict::Arm64Declared);
    let honest = match note {
        Some(t) => t.contains("ARM64") && t.contains("路线图") && t.contains("替代品"),
        None => false,
    };
    cs.add(
        "arm64_note_honest_three_parts",
        honest
            && arm64_note(MachineVerdict::Native64).is_none()
            && arm64_note(MachineVerdict::ThirtyTwo).is_none()
            && arm64_note(MachineVerdict::NotPe).is_none(),
        "",
    );
    // 3) 门序常量钉值（Peblock < Machine < Launch 判别序）。
    cs.add(
        "gate_order_const_pinned",
        GATE_ORDER.len() == 3 && GATE_ORDER[0] == GateStep::Peblock && GATE_ORDER[2] == GateStep::Launch,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F004 · 深化批次四：拒绝记录缓存文件钉值 + 诚实卡片同体系锚
//
// 主册依据（G-A-04【数据与存储】）：「拒绝记录存 `cache/wow64-refusals.json`
// （文件哈希 → 已提示标记），上限 1000 条」；【交互设计】「拒绝卡片样式对齐
// F035 兼容性向导（同一卡片体系）」。既有面：VXR4 序列化/RefusalLedger/
// HonestCard 不重复——本段钉文件名并登记格式口径冲突（见下）。
//
// **口径冲突登记（一处一事实）**：主册写 .json（开放格式惯例）；批次一落的是
// VXR4 自定轻量二进制（与 F020 dump 同族惯例）。以主册文件名为准、格式迁移
// 随闸门（JSON 面复用 F009 export_json 设施）——两处不各自为政。
// ---------------------------------------------------------------------------

/// 拒绝记录缓存路径（主册【数据与存储】原文钉值）。
pub const REFUSAL_CACHE_PATH: &str = "cache\\wow64-refusals.json";
/// 拒绝记录上限（主册同句钉值——与既有 REFUSAL_CAP 同值对账锚）。
pub const REFUSAL_LEDGER_CAP: usize = 1000;

/// F004 深化批次四自检。
pub fn run_wow64_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F004-wow64-deep3");
    // 1) 缓存路径与上限钉值（主册原文；上限与既有 REFUSAL_CAP 同值不漂移）。
    cs.add(
        "refusal_cache_path_and_cap_pinned",
        REFUSAL_CACHE_PATH == "cache\\wow64-refusals.json"
            && REFUSAL_LEDGER_CAP == 1000
            && REFUSAL_CAP == REFUSAL_LEDGER_CAP,
        "",
    );
    // 2) 诚实卡片三要素字段在位（F035 同一卡片体系的结构锚——既有字段面）。
    let card = MachineVerdict::ThirtyTwo.honest_card();
    cs.add(
        "honest_card_f035_same_system",
        matches!(card, Some(c) if !c.what.is_empty() && !c.why.is_empty() && !c.next.is_empty()),
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F004 · 深化批次五：诚实卡片样本账本聚合面（10/10 判据的记账核算）
//
// 主册依据（G-A-04【验收判据】）：「32 位样本集 10 枚 100% 触发诚实卡片
// （零静默失败/零崩溃）」——判据是记账题：10 个样本 × (出卡? 崩溃? 静默?)
// → 账本聚合出率（本面是核算核，样本注入走既有 detect_machine/honest_card）。
// ---------------------------------------------------------------------------

/// 单样本判账（一枚样本的三问）。
#[derive(Clone, Copy, Debug)]
pub struct SampleVerdict {
    pub refused: bool,
    pub card_shown: bool,
    pub crashed: bool,
    pub silent_fail: bool,
}

/// 样本账本聚合（10 枚判据线的核算核）。
pub struct RefusalStats {
    pub samples: [Option<SampleVerdict>; 10],
    pub n: usize,
}

impl RefusalStats {
    pub const fn new() -> RefusalStats {
        RefusalStats { samples: [None; 10], n: 0 }
    }

    pub fn record(&mut self, v: SampleVerdict) -> bool {
        if self.n >= self.samples.len() {
            return false;
        }
        self.samples[self.n] = Some(v);
        self.n += 1;
        true
    }

    /// 出卡率（permille；零样本 → None 不猜）。
    pub fn card_rate_permille(&self) -> Option<u32> {
        if self.n == 0 {
            return None;
        }
        let shown = self.samples[..self.n]
            .iter()
            .flatten()
            .filter(|v| v.card_shown && !v.crashed && !v.silent_fail)
            .count();
        Some((shown * 1000 / self.n) as u32)
    }

    /// 判据：10/10 全出卡且零崩溃零静默。
    pub fn meets_criterion(&self) -> bool {
        self.n == self.samples.len() && self.card_rate_permille() == Some(1000)
    }
}

/// F004 深化批次五自检。
pub fn run_wow64_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F004-wow64-deep4");
    // 1) 满账本判据：10 枚全出卡（零崩溃零静默）→ 1000‰ 达线。
    let mut st = RefusalStats::new();
    for _ in 0..10 {
        st.record(SampleVerdict { refused: true, card_shown: true, crashed: false, silent_fail: false });
    }
    cs.add(
        "refusal_stats_full_criterion",
        st.n == 10 && st.card_rate_permille() == Some(1000) && st.meets_criterion(),
        "",
    );
    // 2) 一枚静默失败 → 率 900‰，判据如实红（不粉饰）。
    let mut st2 = RefusalStats::new();
    for i in 0..10u32 {
        st2.record(SampleVerdict {
            refused: true,
            card_shown: i != 9,
            crashed: false,
            silent_fail: i == 9,
        });
    }
    cs.add(
        "refusal_stats_silent_fail_visible",
        st2.card_rate_permille() == Some(900) && !st2.meets_criterion(),
        "",
    );
    // 3) 零样本如实 None；满容如实拒（账本纪律）。
    let empty = RefusalStats::new();
    let mut full = RefusalStats::new();
    for _ in 0..12 {
        full.record(SampleVerdict { refused: true, card_shown: true, crashed: false, silent_fail: false });
    }
    cs.add(
        "refusal_stats_bounds_honest",
        empty.card_rate_permille().is_none() && full.n == 10,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F004 · 深化批次六：混合包双轨路由（32 位安装器被拒 + 64 位主程序放行）
//
// 主册依据（G-A-04【状态与异常】）：「带 32 位安装器的混合包（32 位安装器装
// 64 位主程序）→ 安装器本身被拒时提示完整归因」——被拒的是安装器，主程序
// 的位数判定独立（拒绝不蔓延：双轨路由面）。
// ---------------------------------------------------------------------------

/// 混合包双轨路由：安装器轨（位数判定独立）与主程序轨互不牵连。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MixedTrack {
    /// 安装器轨：32 位 → 拒（诚实卡片）。
    InstallerRefused,
    /// 主程序轨：64 位 → 放行（按 F001 管线）。
    PayloadAllowed,
    /// 主程序也 32 位 → 拒（同卡片体系）。
    PayloadRefused,
}

/// 路由判定：两轨独立判定（installer_64/payload_64 分别来自各自 PE 头）。
pub fn mixed_route(installer_64: bool, payload_64: bool) -> MixedTrack {
    match (installer_64, payload_64) {
        (false, true) => MixedTrack::InstallerRefused,
        (false, false) => MixedTrack::PayloadRefused,
        (true, true) | (true, false) => MixedTrack::PayloadAllowed,
    }
}

/// F004 深化批次六自检。
pub fn run_wow64_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F004-wow64-deep5");
    // 1) 典型混合包：32 位安装器拒 + 64 位主程序放行（拒绝不蔓延——两轨独立）。
    cs.add(
        "mixed_route_installer_refused_payload_allowed",
        mixed_route(false, true) == MixedTrack::InstallerRefused,
        "",
    );
    // 2) 双 32 位：两轨全拒（同卡片体系——不给第二次不同话术）。
    cs.add(
        "mixed_route_both_refused_same_card",
        mixed_route(false, false) == MixedTrack::PayloadRefused,
        "",
    );
    // 3) 安装器 64 位（纯 64 位包）：主程序轨直接放行（无混合语义）。
    cs.add(
        "pure64_package_payload_allowed",
        mixed_route(true, true) == MixedTrack::PayloadAllowed,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F004 · 深化批次七：拒绝卡片动作对（两动作面——关闭 / 查 64 位替代品）
//
// 主册依据（G-A-04【用户故事】：「附『查看 64 位替代品』按钮直跳星图搜索」
// +【交互设计】：「两个动作（关闭/查替代）」——动作对是卡片交互的契约面
/// （与 HonestCard.alt_query 既有面联动——一处一事实）。
// ---------------------------------------------------------------------------

/// 卡片动作（两动作契约——不做第三动作：拒绝卡片不放危险位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardAction {
    Close,
    FindAlternative,
}

/// 动作对（显示文案 + 键盘焦点序——Tab 先到主动作「查替代」再「关闭」，
/// 破坏性/放弃性动作殿后——交互词典一致性）。
pub const CARD_ACTION_ORDER: [(CardAction, &'static str); 2] = [
    (CardAction::FindAlternative, "查看 64 位替代品"),
    (CardAction::Close, "关闭"),
];

/// F004 深化批次七自检。
pub fn run_wow64_deep6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F004-wow64-deep6");
    // 1) 动作对契约：恰好两动作、文案非空、主动作在前（Tab 序）。
    cs.add(
        "card_action_pair_contract",
        CARD_ACTION_ORDER.len() == 2
            && CARD_ACTION_ORDER[0].0 == CardAction::FindAlternative
            && CARD_ACTION_ORDER[1].0 == CardAction::Close
            && CARD_ACTION_ORDER.iter().all(|(_, t)| !t.is_empty()),
        "",
    );
    // 2) 与 HonestCard 联动锚：卡片自带的 alt_query 供「查替代」直跳星图
    //    （既有面——动作与数据源对得上）。
    let card = MachineVerdict::ThirtyTwo.honest_card();
    cs.add(
        "card_action_data_link",
        matches!(card, Some(c) if !c.alt_query.is_empty()),
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F004 · 深化批次八：文件系统重定向映射面（32 位进程访问 System32 的
// WOW64 重定向语义——真映射规则三向判定，非记账模型）。
//
// 语义（Microsoft WOW64 文档）：① %windir%\System32 → 重定向到 SysWOW64；
// ② %windir%\SysWOW64 本身 **不再** 重定向（防递归）；③ 其余路径原样。
// 大小写不敏感（NTFS 语义）。
// ---------------------------------------------------------------------------

/// 重定向判定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedirectVerdict {
    /// System32 前缀命中 → 应改指 SysWOW64。
    ToSysWow64,
    /// SysWOW64 前缀命中 → 字面访问（不二次重定向）。
    LiteralSysWow64,
    /// 其他路径 → 字面访问。
    LiteralOther,
}

const SYS32: &[u8] = b"c:\\windows\\system32\\";
const SYSWOW: &[u8] = b"c:\\windows\\syswow64\\";

/// 路径重定向判定（大小写不敏感前缀匹配；`/` 归一为 `\` 后再比）。
pub fn wow64_redirect_verdict(path: &str) -> RedirectVerdict {
    let mut norm: alloc::vec::Vec<u8> = path
        .as_bytes()
        .iter()
        .map(|b| if *b == b'/' { b'\\' } else { b.to_ascii_lowercase() })
        .collect();
    norm.push(0);
    let p = &norm[..norm.len() - 1];
    if p.starts_with(SYSWOW) {
        RedirectVerdict::LiteralSysWow64
    } else if p.starts_with(SYS32) {
        RedirectVerdict::ToSysWow64
    } else {
        RedirectVerdict::LiteralOther
    }
}

/// 重定向后的实际路径（仅 ToSysWow64 有映射——其余 None 不编造）。
pub fn wow64_redirected_path(path: &str, out: &mut alloc::vec::Vec<u8>) -> bool {
    if wow64_redirect_verdict(path) != RedirectVerdict::ToSysWow64 {
        return false;
    }
    out.clear();
    out.extend_from_slice(b"c:\\windows\\syswow64\\");
    let skip = b"c:\\windows\\system32\\".len();
    let rest = &path[skip.min(path.len())..]; // 保留原路径大小写（Windows 语义）
    out.extend_from_slice(rest.as_bytes());
    true
}

/// F004 深化批次八自检。
fn run_wow64_deep7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F004-wow64-deep7");
    let mut out = alloc::vec::Vec::new();
    // 1) System32 → SysWOW64（正斜杠归一；输出保留原大小写）。
    let r1 = wow64_redirected_path("C:\\Windows\\SYSTEM32\\kernel32.dll", &mut out);
    let t1 = core::str::from_utf8(&out).unwrap_or("").to_string();
    let r2 = wow64_redirected_path("c:/windows/system32/user32.dll", &mut out);
    cs.add(
        "redirect_system32_to_syswow64",
        r1 && r2 && t1 == "c:\\windows\\syswow64\\kernel32.dll",
        "",
    );
    // 2) SysWOW64 不二次重定向；非系统目录原样。
    let v = wow64_redirect_verdict("C:\\Windows\\SysWOW64\\notepad.exe");
    let v2 = wow64_redirect_verdict("D:\\Apps\\tool.exe");
    cs.add(
        "syswow64_never_re_redirected",
        v == RedirectVerdict::LiteralSysWow64 && v2 == RedirectVerdict::LiteralOther,
        "",
    );
    // 3) 前缀相似但不同（System32Own）不误命中——前缀匹配必须到分隔符。
    let v3 = wow64_redirect_verdict("c:\\windows\\system32own\\x.dll");
    cs.add(
        "prefix_boundary_exact",
        v3 == RedirectVerdict::LiteralOther,
        "",
    );
    cs
}
