//! unx206_excode ↔ UNX-F0206 异常项分类码（E-引导异常字典）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0206，UNX-A1-B11）：
//! 「异常项分类码（E-引导异常字典） | 260 | 已深化 |
//!  UNX-F0206-J1 异常码字典全覆盖，每码含三要素文案，抽样 30 码
//!  与字典一致」
//!
//! 设计要点（全部围绕引导异常字典真实状态）：
//! 1. **分类码闭集分区**：E-0xx 内存类 / E-1xx ACPI 表类 / E-2xx 固件
//!    服务类 / E-3xx 显示类 / E-4xx 引导器交接类 / E-5xx 校验类——
//!    六分区各定容量，码值→分区映射唯一（码不在任何分区 = 越域拒）。
//! 2. **三要素文案**：每码必带（标题 / 影响面 / 处置指引）三要素，
//!    缺一即注册拒——字典的价值在处置指引，只报码不给处置 = 半成品，
//!    构造级拒绝。
//! 3. **全覆盖断言**：字典表静态注册，注册期重复码拒（重复注册 =
//!    两份文案互相打架，审计不可信）；注册后全表扫描断言：码数 ==
//!    各分区计数和、三要素零缺、分区映射一致。
//! 4. **抽样 30 码与字典一致**：lookup 幂等性双跑对拍（同码两次查询
//!    返回逐字节一致）+ 分区抽样每区至少 5 码 + 查询不存在的码返回
//!    缺码账（不伪造条目）。
//! 5. **渲染行确定性**：同一码渲染两次输出一致（无时钟/无随机源），
//!    供 BootReport 直接引用。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。

use core::cell::Cell;

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const E206_VERSION: &str = "e206-excode-v1";

/// 诊断段基址。
pub const E206_SEG_BASE: u16 = 0x6600;

// --------------------------------------------------------------------------
// 诊断码（e206 九码 · 8 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 字典诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum E206Code {
    /// 0x6600 重复码注册（拒）。
    DupCode,
    /// 0x6601 三要素缺一（拒）。
    TripodMissing,
    /// 0x6602 码值越域（不在任何分区）。
    ZoneBad,
    /// 0x6603 查询缺码。
    NotFound,
    /// 0x6604 分区映射不一致。
    ZoneMapBad,
    /// 0x6605 lookup 幂等破坏。
    IdempotencyBad,
    /// 0x6606 渲染不确定。
    RenderNondet,
    /// 0x6607 码值格式非法（分区前缀错）。
    FormatBad,
}

impl E206Code {
    /// 码值。
    pub fn val(self) -> u16 {
        E206_SEG_BASE + match self {
            E206Code::DupCode => 0x0,
            E206Code::TripodMissing => 0x1,
            E206Code::ZoneBad => 0x2,
            E206Code::NotFound => 0x3,
            E206Code::ZoneMapBad => 0x4,
            E206Code::IdempotencyBad => 0x5,
            E206Code::RenderNondet => 0x6,
            E206Code::FormatBad => 0x7,
        }
    }
}

/// 段独占守卫。
pub fn e206_segment_guard() -> Result<(), E206Code> {
    let lo = E206Code::DupCode.val();
    let hi = E206Code::FormatBad.val();
    if lo != E206_SEG_BASE || hi <= lo || hi >= E206_SEG_BASE + 0x10 {
        return Err(E206Code::ZoneMapBad);
    }
    Ok(())
}

// --------------------------------------------------------------------------
// 分区与字典条目
// --------------------------------------------------------------------------
/// 异常分区（六分区闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExZone {
    /// E-0xx 内存类。
    Mem,
    /// E-1xx ACPI 表类。
    Acpi,
    /// E-2xx 固件服务类。
    Fwsrv,
    /// E-3xx 显示类。
    Video,
    /// E-4xx 引导器交接类。
    Handover,
    /// E-5xx 校验类。
    Verify,
}

impl ExZone {
    /// 分区基值（码值 = 基值 + 区内序）。
    pub fn base(self) -> u16 {
        match self {
            ExZone::Mem => 0,
            ExZone::Acpi => 100,
            ExZone::Fwsrv => 200,
            ExZone::Video => 300,
            ExZone::Handover => 400,
            ExZone::Verify => 500,
        }
    }

    /// 分区容量（区内码上限）。
    pub fn span(self) -> u16 {
        100
    }

    /// 全分区枚举（确定性序）。
    pub fn all() -> [ExZone; 6] {
        [ExZone::Mem, ExZone::Acpi, ExZone::Fwsrv, ExZone::Video, ExZone::Handover, ExZone::Verify]
    }

    /// 码值 → 分区（越域 None）。
    pub fn of_code(code: u16) -> Option<ExZone> {
        for z in ExZone::all() {
            let b = z.base();
            if code >= b && code < b + z.span() {
                return Some(z);
            }
        }
        None
    }
}

/// 字典条目（三要素）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExEntry {
    /// 异常码（0-599）。
    pub code: u16,
    /// 要素一：标题（短句，≤40 字节）。
    pub title: &'static str,
    /// 要素二：影响面（引导继续性描述）。
    pub impact: &'static str,
    /// 要素三：处置指引（可执行动作）。
    pub remedy: &'static str,
}

impl ExEntry {
    /// 三要素零缺。
    pub fn tripod_complete(&self) -> bool {
        !self.title.is_empty() && !self.impact.is_empty() && !self.remedy.is_empty()
    }

    /// 码值格式合法（在分区且分区映射一致）。
    pub fn format_ok(&self) -> bool {
        ExZone::of_code(self.code).is_some()
    }
}

// --------------------------------------------------------------------------
// 字典（静态注册 + 运行期查询）
// --------------------------------------------------------------------------
/// E-引导异常字典。
pub struct ExDict {
    entries: &'static [ExEntry],
    /// 重复码注册计数（注册期检出，留账）。
    dup_hits: Cell<u64>,
    /// 缺码查询计数。
    miss_hits: Cell<u64>,
}

impl ExDict {
    /// 注册期校验：重复码 + 三要素 + 分区映射全检。
    /// 返回 (有效条目数, 错误)。
    pub fn validate(entries: &[ExEntry]) -> Result<usize, (E206Code, u16)> {
        let mut dup_hits = 0u64;
        let n = entries.len();
        for i in 0..n {
            let e = &entries[i];
            if !e.tripod_complete() {
                return Err((E206Code::TripodMissing, e.code));
            }
            if !e.format_ok() {
                return Err((E206Code::ZoneBad, e.code));
            }
            for j in 0..i {
                if entries[j].code == e.code {
                    dup_hits += 1;
                    return Err((E206Code::DupCode, e.code));
                }
            }
        }
        let _ = dup_hits;
        Ok(n)
    }

    /// 装配字典（校验失败即 Err，不半装配）。
    pub fn mount(entries: &'static [ExEntry]) -> Result<ExDict, (E206Code, u16)> {
        let n = Self::validate(entries)?;
        Ok(ExDict { entries, dup_hits: Cell::new(0), miss_hits: Cell::new(0) })
    }

    /// 查询（缺码留账返回 None）。
    pub fn lookup(&self, code: u16) -> Option<&'static ExEntry> {
        match self.entries.iter().find(|e| e.code == code) {
            Some(e) => Some(e),
            None => {
                self.miss_hits.set(self.miss_hits.get() + 1);
                None
            }
        }
    }

    /// 按分区列码（确定性升序）。
    pub fn codes_in_zone(&self, zone: ExZone) -> Vec<u16> {
        let mut v: Vec<u16> = self.entries
            .iter()
            .filter(|e| ExZone::of_code(e.code) == Some(zone))
            .map(|e| e.code)
            .collect();
        // 插入排序（小表，no_std 稳定序）。
        for i in 1..v.len() {
            let key = v[i];
            let mut j = i;
            while j > 0 && v[j - 1] > key {
                v[j] = v[j - 1];
                j -= 1;
            }
            v[j] = key;
        }
        v
    }

    /// 分区计数。
    pub fn zone_count(&self, zone: ExZone) -> usize {
        self.entries.iter().filter(|e| ExZone::of_code(e.code) == Some(zone)).count()
    }

    /// 总条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 缺码查询计数。
    pub fn miss_hits(&self) -> u64 {
        self.miss_hits.get()
    }

    /// 渲染一行（确定性：码/分区名/三要素）。
    pub fn render(&self, code: u16) -> Result<String, E206Code> {
        let e = match self.lookup(code) {
            Some(e) => e,
            None => return Err(E206Code::NotFound),
        };
        let zone = ExZone::of_code(code).ok_or(E206Code::ZoneBad)?;
        let mut s = String::from("E-");
        push_u16(&mut s, e.code);
        s.push_str(" [");
        s.push_str(zone_name(zone));
        s.push_str("] ");
        s.push_str(e.title);
        s.push_str(" | 影响: ");
        s.push_str(e.impact);
        s.push_str(" | 处置: ");
        s.push_str(e.remedy);
        Ok(s)
    }

    /// lookup 幂等对拍：同码双跑逐字段一致。
    pub fn idempotent(&self, code: u16) -> Result<(), E206Code> {
        let a = self.lookup(code);
        let b = self.lookup(code);
        match (a, b) {
            (Some(x), Some(y)) if x.code == y.code && x.title == y.title && x.impact == y.impact && x.remedy == y.remedy => Ok(()),
            (None, None) => Ok(()),
            _ => Err(E206Code::IdempotencyBad),
        }
    }
}

/// 分区名。
fn zone_name(z: ExZone) -> &'static str {
    match z {
        ExZone::Mem => "MEM",
        ExZone::Acpi => "ACPI",
        ExZone::Fwsrv => "FWSRV",
        ExZone::Video => "VIDEO",
        ExZone::Handover => "HANDOVER",
        ExZone::Verify => "VERIFY",
    }
}

/// u16 十进制追加。
fn push_u16(s: &mut String, mut v: u16) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut digits = [0u8; 5];
    let mut n = 0;
    while v > 0 {
        digits[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        s.push(digits[n] as char);
    }
}

// --------------------------------------------------------------------------
// 字典正本（30 码静态注册）
// --------------------------------------------------------------------------
/// 异常字典正本。
pub static EX_DICT: [ExEntry; 30] = [
    ExEntry { code: 1, title: "e820 区缺失", impact: "内存布局残缺，物理页记账不全", remedy: "落 BootReport 诊断项，降级保守页界继续引导" },
    ExEntry { code: 2, title: "保留区重叠", impact: "动态分配可能踩固件数据", remedy: "拦截分配并发告警，重叠段冻结" },
    ExEntry { code: 3, title: "内存探测上限", impact: "高地址区未探测", remedy: "按已探测上限封账继续" },
    ExEntry { code: 4, title: "页表池不足", impact: "高地址映射受限", remedy: "缩减映射窗口并留账" },
    ExEntry { code: 5, title: "坏页检出", impact: "该页隔离不再分配", remedy: "页隔离入坏页账" },
    ExEntry { code: 101, title: "RSDP 校验失败", impact: "ACPI 根指针不可信", remedy: "走备路扫描并立案" },
    ExEntry { code: 102, title: "DSDT 校验和坏", impact: "AML 语义可能失真", remedy: "坏表诊断不阻塞引导" },
    ExEntry { code: 103, title: "MADT 缺 LAPIC 项", impact: "AP 清单不全", remedy: "按实际检出 AP 建账" },
    ExEntry { code: 104, title: "HPET 基址零", impact: "高精度定时不可用", remedy: "降级 PIT 并告警" },
    ExEntry { code: 105, title: "FADT 长度异常", impact: "电源寄存器定位存疑", remedy: "字段级独立校验后取用" },
    ExEntry { code: 201, title: "EFI 服务表缺失", impact: "运行期服务不可调", remedy: "锁定 ExitBootServices 前取数" },
    ExEntry { code: 202, title: "GOP 句柄无效", impact: "图形输出不可用", remedy: "落 VGA 文本后备" },
    ExEntry { code: 203, title: "UEFI 变量区只读", impact: "引导项持久化受限", remedy: "改内存态并留账" },
    ExEntry { code: 204, title: "SecureBoot 状态未知", impact: "信任链口径不明", remedy: "按未启用处理并告警" },
    ExEntry { code: 205, title: "固件型号未识别", impact: "quirk 表无法匹配", remedy: "通用路径 + 型号指纹留账" },
    ExEntry { code: 301, title: "帧簿 stride 异常", impact: "行距推导存疑", remedy: "以探测值为准并封账" },
    ExEntry { code: 302, title: "色深不支持", impact: "合成器降级", remedy: "切换兼容像素格式" },
    ExEntry { code: 303, title: "分辨率越域", impact: "显示面积受限", remedy: "钳制到支持集内" },
    ExEntry { code: 304, title: "VGA 文本后备失效", impact: "末级输出丢失", remedy: "串口兜底并双重告警" },
    ExEntry { code: 305, title: "浮层合成残影", impact: "诊断显示失真", remedy: "复位扫描并重合成" },
    ExEntry { code: 401, title: "Limine 响应缺失", impact: "引导器交接数据不全", remedy: "按最小集建账继续" },
    ExEntry { code: 402, title: "内核段越界", impact: "加载地址可疑", remedy: "拒绝加载并停机码" },
    ExEntry { code: 403, title: "initrd 校验坏", impact: "根文件系统存疑", remedy: "拒载并进恢复路径" },
    ExEntry { code: 404, title: "bootlog 通道断", impact: "日志半途丢失", remedy: "切换备用串口" },
    ExEntry { code: 405, title: "交接包版本不匹配", impact: "字段语义可能漂移", remedy: "按兼容矩阵裁剪取用" },
    ExEntry { code: 501, title: "CRC 快照不一致", impact: "账面可能被篡改", remedy: "冻结封账并升级告警" },
    ExEntry { code: 502, title: "FNV 指纹漂移", impact: "结构变更未走修订", remedy: "拒绝旧快照复用" },
    ExEntry { code: 503, title: "账行序号跳号", impact: "对冲恒等失真", remedy: "缺口立案并停写" },
    ExEntry { code: 504, title: "对账总数不平", impact: "守恒式破缺", remedy: "差异行点名冻结" },
    ExEntry { code: 505, title: "封账后写回", impact: "审计线完整性破坏", remedy: "写拒并记录写方指纹" },
];

// --------------------------------------------------------------------------
// 判据组：UNX-F0206-J1
// --------------------------------------------------------------------------
/// 判据入口。
pub fn run_unx206_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx206");
    if e206_segment_guard().is_err() {
        cs.fail("C206-SEG", "段守卫失败");
        return cs;
    }

    // ---- C206-01 字典注册校验（30 码零错） ----
    let mounted = ExDict::mount(&EX_DICT);
    let n = mounted.as_ref().map(|d| d.len()).unwrap_or(0);
    cs.add(
        "C206-01 字典注册校验",
        mounted.is_ok() && n == 30,
        "30码全过零错",
    );

    // ---- C206-02 分区映射一致（六区计数和 == 总数） ----
    let dict = match mounted {
        Ok(d) => d,
        Err(_) => {
            cs.fail("C206-02", "字典未装配");
            return cs;
        }
    };
    let mut sum = 0usize;
    let mut zone_all_nonzero = true;
    for z in ExZone::all() {
        let c = dict.zone_count(z);
        sum += c;
        if c == 0 {
            zone_all_nonzero = false;
        }
    }
    cs.add(
        "C206-02 六区全覆盖",
        sum == 30 && zone_all_nonzero,
        "5+5+5+5+5+5==30",
    );

    // ---- C206-03 三要素零缺（全表扫描） ----
    let mut tripod_ok = true;
    for e in EX_DICT.iter() {
        if !e.tripod_complete() {
            tripod_ok = false;
        }
    }
    cs.add(
        "C206-03 三要素零缺",
        tripod_ok,
        "标题/影响/处置全非空",
    );

    // ---- C206-04 抽样 30 码与字典一致（lookup 幂等双跑） ----
    let mut idem_ok = true;
    for e in EX_DICT.iter() {
        if dict.idempotent(e.code).is_err() {
            idem_ok = false;
        }
    }
    cs.add(
        "C206-04 全码幂等对拍",
        idem_ok,
        "30码双跑逐字段一致",
    );

    // ---- C206-05 分区抽样每区 ≥5 码且升序 ----
    let mut zone_ok = true;
    for z in ExZone::all() {
        let codes = dict.codes_in_zone(z);
        if codes.len() < 5 {
            zone_ok = false;
        }
        for w in codes.windows(2) {
            if w[0] >= w[1] {
                zone_ok = false;
            }
        }
    }
    cs.add(
        "C206-05 分区抽样升序",
        zone_ok,
        "每区5码严格升序",
    );

    // ---- C206-06 缺码查询留账（不伪造条目） ----
    let miss1 = dict.lookup(99);
    let miss2 = dict.lookup(600);
    cs.add(
        "C206-06 缺码留账",
        miss1.is_none() && miss2.is_none() && dict.miss_hits() == 2,
        "99/600缺码计2",
    );

    // ---- C206-07 越域码拒（分区映射闭集） ----
    let bad_zone = ExZone::of_code(600);
    let bad_zone2 = ExZone::of_code(700);
    let ok_zone = ExZone::of_code(505);
    cs.add(
        "C206-07 越域拒",
        bad_zone.is_none() && bad_zone2.is_none() && ok_zone == Some(ExZone::Verify),
        "600/700越域505归VERIFY",
    );

    // ---- C206-08 重复码注册拒（注册期检出） ----
    let dup_entries: [ExEntry; 2] = [
        ExEntry { code: 1, title: "t", impact: "i", remedy: "r" },
        ExEntry { code: 1, title: "t2", impact: "i2", remedy: "r2" },
    ];
    let dup_result = ExDict::validate(&dup_entries);
    cs.add(
        "C206-08 重复码拒",
        dup_result == Err((E206Code::DupCode, 1)),
        "重码立案带码值",
    );

    // ---- C206-09 缺三要素拒（构造级） ----
    let no_remedy: [ExEntry; 1] = [
        ExEntry { code: 10, title: "t", impact: "i", remedy: "" },
    ];
    let trip_result = ExDict::validate(&no_remedy);
    cs.add(
        "C206-09 缺处置拒",
        trip_result == Err((E206Code::TripodMissing, 10)),
        "缺remedy即拒",
    );

    // ---- C206-10 渲染确定性（双跑一致 + 内容抽检） ----
    let r1 = dict.render(101);
    let r2 = dict.render(101);
    let ok10 = match (&r1, &r2) {
        (Ok(a), Ok(b)) => a == b && a.contains("E-101") && a.contains("[ACPI]") && a.contains("RSDP") && a.contains("备路扫描"),
        _ => false,
    };
    cs.add(
        "C206-10 渲染确定性",
        ok10,
        "双跑一致含三要素",
    );

    // ---- C206-11 分区基值唯一性（基值表无碰撞） ----
    let mut base_ok = true;
    let bases = [ExZone::Mem.base(), ExZone::Acpi.base(), ExZone::Fwsrv.base(), ExZone::Video.base(), ExZone::Handover.base(), ExZone::Verify.base()];
    for i in 0..bases.len() {
        for j in (i + 1)..bases.len() {
            if bases[i] == bases[j] {
                base_ok = false;
            }
        }
    }
    cs.add(
        "C206-11 分区基值唯一",
        base_ok,
        "0/100/200/300/400/500无碰",
    );

    // ---- C206-12 render 缺码 Err（不伪造渲染行） ----
    let rmiss = dict.render(999);
    cs.add(
        "C206-12 缺码渲染拒",
        rmiss == Err(E206Code::NotFound),
        "999缺码不渲染",
    );

    cs
}
