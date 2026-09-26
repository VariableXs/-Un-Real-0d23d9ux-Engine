
// ---------------------------------------------------------------------------
// F001 · 深化批次三：签名嗅探（MZ+PE 双验证，扩展名不作准）+ 三入口同管线
// 登记面 + 装载失败归因五分类（F035 错误卡）
//
// 主册依据（G-A-01【设计细节】）：「双击识别走文件签名嗅探（MZ 头加 PE 标记
// 双验证，扩展名不作准）」；【交互设计】「入口共三处：资源管理器双击/桌面
// 双击/开始菜单搜索后 Enter——三条路走同一装载服务，不许有第二条实现」；
// 【状态与异常】③「装载中崩溃 → 占位窗转为错误卡（展开显示归因五分类 F035）」。
// ---------------------------------------------------------------------------

/// 签名嗅探判定（MZ + PE 双验证——文件内容是唯一裁判，.exe 扩展名不作准）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SniffVerdict {
    /// MZ 头 + e_lfanew 处 PE\0\0 双验齐 → 进装载管线。
    PeExecutable,
    /// 有 MZ 无 PE 标记 → 非 PE（三要素对话框：「这不是 Windows 可执行文件」）。
    MzWithoutPe,
    /// 连 MZ 都没有 → 直接归因「非 Windows 可执行文件」。
    NotExecutable,
    /// 短于 DOS 头（0x40 字节）读不到双验证材料——按非可执行归因，独立计数。
    Truncated,
}

/// DOS 头长度（e_lfanew 字段所在结构的固定尺寸）。
const SNIFF_DOS_HDR: usize = 0x40;

/// 双验证嗅探：先验 MZ，再沿 e_lfanew（DOS 头 0x3C，小端 u32）验 PE 签名。
/// e_lfanew 指向越界 = PE 标记区读不到 = 双验证失败（不给「可能是」的假象）。
pub fn sniff_signature(data: &[u8]) -> SniffVerdict {
    if data.len() < SNIFF_DOS_HDR {
        return SniffVerdict::Truncated;
    }
    if data[0] != b'M' || data[1] != b'Z' {
        return SniffVerdict::NotExecutable;
    }
    let e_lfanew =
        u32::from_le_bytes([data[0x3C], data[0x3D], data[0x3E], data[0x3F]]) as usize;
    if e_lfanew < SNIFF_DOS_HDR || e_lfanew + 4 > data.len() {
        // e_lfanew 指回头部内部或越出文件：PE 标记不可达，双验证判负。
        return SniffVerdict::MzWithoutPe;
    }
    if data[e_lfanew] == b'P'
        && data[e_lfanew + 1] == b'E'
        && data[e_lfanew + 2] == 0
        && data[e_lfanew + 3] == 0
    {
        SniffVerdict::PeExecutable
    } else {
        SniffVerdict::MzWithoutPe
    }
}

/// 装载入口三处（主册【交互设计】：资源管理器/桌面/开始菜单搜索 Enter）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum EntryDoor {
    Explorer = 0,
    Desktop = 1,
    StartMenuEnter = 2,
}

impl EntryDoor {
    /// 登记名（审计/录屏归因用）。
    pub fn as_str(self) -> &'static str {
        match self {
            EntryDoor::Explorer => "explorer",
            EntryDoor::Desktop => "desktop",
            EntryDoor::StartMenuEnter => "startmenu-enter",
        }
    }
}

/// 入口审计：三入口计数面——语义上三处必须汇入同一 [`LaunchPipeline`]
/// （不许有第二条实现），本结构只做「谁从哪个门进来」的可观测登记。
#[derive(Clone, Copy)]
pub struct EntryAudit {
    seen: [u32; 3],
    total: u32,
}

impl EntryAudit {
    pub const fn new() -> EntryAudit {
        EntryAudit { seen: [0; 3], total: 0 }
    }

    pub fn record(&mut self, door: EntryDoor) {
        self.seen[door as usize] += 1;
        self.total += 1;
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    pub fn per_door(&self, door: EntryDoor) -> u32 {
        self.seen[door as usize]
    }
}

/// 装载失败归因五分类（F035——错误卡展开页逐类短语，异常零静默落点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Attribution5 {
    /// 格式不是 PE（对应异常①）。
    NotPe = 0,
    /// peblock 拒绝——F037 完整解释与越权入口（对应异常②）。
    PeblockDenied = 1,
    /// 装载中崩溃/PE 结构异常（对应异常③，F175 隔离联动）。
    BadFormat = 2,
    /// 内存不足（对应 F002 诚实提示，不闪退）。
    ResourceShort = 3,
    /// 窗口子系统未能启动（GUI/CUI 两类之外无承诺面）。
    WindowSubsystemFail = 4,
}

impl Attribution5 {
    /// 归因短语（三要素之「发生了什么+为什么」；「下一步」统一指向 F035 向导，
    /// 由错误卡外壳统一附上——一处一事实）。
    pub fn as_str(self) -> &'static str {
        match self {
            Attribution5::NotPe => "这不是 Windows 可执行文件",
            Attribution5::PeblockDenied => "程序被安全门拒绝（详见越权说明）",
            Attribution5::BadFormat => "文件损坏或 PE 结构异常，装载中止",
            Attribution5::ResourceShort => "内存不足，无法完成装载",
            Attribution5::WindowSubsystemFail => "程序窗口子系统未能启动",
        }
    }

    /// 从分类序号取枚举（诊断面/账本回放用；越界返回 None 不猜）。
    pub fn from_index(i: u8) -> Option<Attribution5> {
        match i {
            0 => Some(Attribution5::NotPe),
            1 => Some(Attribution5::PeblockDenied),
            2 => Some(Attribution5::BadFormat),
            3 => Some(Attribution5::ResourceShort),
            4 => Some(Attribution5::WindowSubsystemFail),
            _ => None,
        }
    }
}

/// F001 深化批次三自检。
pub fn run_dblrun_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F001-dblrun-deep2");
    // 1) 签名嗅探：最小 PE（MZ + e_lfanew=0x40 + PE 签名）判 PeExecutable。
    let mut minimal = [0u8; 0x44];
    minimal[0] = b'M';
    minimal[1] = b'Z';
    minimal[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
    minimal[0x40..0x44].copy_from_slice(&[b'P', b'E', 0, 0]);
    // 扩展名不作准的对偶验证：内容齐双验 = 放行；内容无签名 = 拒绝。
    let pe = sniff_signature(&minimal);
    let mut broken = minimal;
    broken[0x40] = b'X'; // PE 签名破坏
    let no_mz: [u8; 8] = [0; 8];
    cs.add(
        "sniff_signature_double_verify",
        pe == SniffVerdict::PeExecutable
            && sniff_signature(&broken) == SniffVerdict::MzWithoutPe
            && sniff_signature(&no_mz) == SniffVerdict::NotExecutable
            && sniff_signature(&minimal[..0x20]) == SniffVerdict::Truncated,
        "",
    );
    // 2) e_lfanew 越界（指向文件尾外）= PE 标记不可达，双验证判负不 panic。
    let mut wild = [0u8; 0x44];
    wild[0] = b'M';
    wild[1] = b'Z';
    wild[0x3C..0x40].copy_from_slice(&0xFF00u32.to_le_bytes());
    cs.add(
        "sniff_e_lfanew_out_of_range_safe",
        sniff_signature(&wild) == SniffVerdict::MzWithoutPe,
        "",
    );
    // 3) 三入口登记：三入口计数独立且合计一致（同一管线前提下的可观测面）。
    let mut audit = EntryAudit::new();
    audit.record(EntryDoor::Explorer);
    audit.record(EntryDoor::Explorer);
    audit.record(EntryDoor::Desktop);
    audit.record(EntryDoor::StartMenuEnter);
    cs.add(
        "entry_doors_same_pipeline_audit",
        audit.total() == 4
            && audit.per_door(EntryDoor::Explorer) == 2
            && audit.per_door(EntryDoor::Desktop) == 1
            && audit.per_door(EntryDoor::StartMenuEnter) == 1
            && EntryDoor::StartMenuEnter.as_str() == "startmenu-enter",
        "",
    );
    // 4) 归因五分类：全类短语非空可回放，序号往返恒等，越界如实 None。
    let mut roundtrip = true;
    let mut phrases_nonempty = true;
    for i in 0..5u8 {
        match Attribution5::from_index(i) {
            Some(v) => {
                phrases_nonempty &= !v.as_str().is_empty();
                roundtrip &= Attribution5::from_index(v as u8) == Some(v);
            }
            None => roundtrip = false,
        }
    }
    cs.add(
        "attribution5_all_classes_roundtrip",
        roundtrip && phrases_nonempty && Attribution5::from_index(5).is_none(),
        "",
    );
    cs
}
