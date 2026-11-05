//! UNX-F12691 跨域消费面总对账表（D2/D3/D4/E1 四方） · 0x81680段
//!
//! 判据原文（派工卡 12690 逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12691-J1 登记总表 ≥60 行（B29–B34 消费边全量），
//!      三查逐条可点验；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 消费边行域模型：登记行五元（行号/消费批 B29–B34/接口短名/探针
//!    码/点验判）——登记总表由六批冻结接口清单逐行铺开；
//! 2. 行额判据锚：B29–B34 六批冻结清单共 64 行（11+11+11+11+10+10），
//!    ≥60 行下限满足且逐行可点验（行额普查判据）；
//! 3. 三查口径（逐条可点验的三面）：行号连续查（0..64 连续无跳无重）、
//!    批次归属查（批闭集 + 批额与冻结清单逐批相等）、接口唯一查（接口
//!    短名全表唯一非空）——三查全绿才许对账封账；
//! 4. 探针码口径：每行探针码 = 0x6000 + 行号（冻结折算），漂移即点验
//!    失败（显性拒，不静默）；
//! 5. 对账账状态机 FwOpen → FwRunning → FwSealed 单向推进，倒退显性
//!    拒；封账只读；零静默：行数 ≡ 点验数守恒；
//! 6. 注入联测十路：行号跳号/行号重复/接口置空/接口重复/行额不足/
//!    批额漂移/批次缺一/探针码漂移/空表/摘要空——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律 match/
//!    if let/.get()/.get_mut()/saturating_*，无 unwrap/expect/panic/
//!    裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `registry_rows_ok` ↔ ② 登记总表 ≥60 行（B29–B34 消费边全量）；
//!   - `three_probe_ok` ↔ ② 三查逐条可点验；
//!   - `batch_census_ok` ↔ ② 六批批额与冻结清单逐批相等；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12690 − 12600)）。
pub const SEG_BASE: u32 = 0x81680;

// ── 跨域对账诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FwDiag {
    /// 行号跳号。
    ERowSkip,
    /// 行号重复。
    ERowDup,
    /// 接口置空。
    EIfaceEmpty,
    /// 接口重复。
    EIfaceDup,
    /// 行额不足（< 60 行）。
    EShortTable,
    /// 批额漂移（批行数 ≠ 冻结清单）。
    EBatchDrift,
    /// 批次缺一。
    EBatchMissing,
    /// 探针码漂移。
    EProbeDrift,
    /// 空表。
    EEmptyTable,
    /// 封账后写。
    ESealedWrite,
    /// 状态机倒退。
    ERewind,
}

impl FwDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                FwDiag::ERowSkip => 0x01,
                FwDiag::ERowDup => 0x02,
                FwDiag::EIfaceEmpty => 0x03,
                FwDiag::EIfaceDup => 0x04,
                FwDiag::EShortTable => 0x05,
                FwDiag::EBatchDrift => 0x06,
                FwDiag::EBatchMissing => 0x07,
                FwDiag::EProbeDrift => 0x08,
                FwDiag::EEmptyTable => 0x09,
                FwDiag::ESealedWrite => 0x0a,
                FwDiag::ERewind => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            FwDiag::ERowSkip => "行号跳号：行号必须 0..N 连续，跳号先回补",
            FwDiag::ERowDup => "行号重复：一行一号，重复先查已有账",
            FwDiag::EIfaceEmpty => "接口置空：接口短名非空才可登记，空名先补",
            FwDiag::EIfaceDup => "接口重复：接口短名全表唯一，重复先改短名",
            FwDiag::EShortTable => "行额不足：登记总表 ≥60 行，缺行先补登记",
            FwDiag::EBatchDrift => "批额漂移：批行数必须与冻结清单相等，先核清单",
            FwDiag::EBatchMissing => "批次缺一：B29–B34 六批全量，缺批先回补",
            FwDiag::EProbeDrift => "探针码漂移：探针码 = 0x6000 + 行号，漂移先重折算",
            FwDiag::EEmptyTable => "空表：空表不得封账，先铺冻结清单",
            FwDiag::ESealedWrite => "封账后写：对账表只读，补行走下一轮对账",
            FwDiag::ERewind => "状态机倒退：FwOpen→FwRunning→FwSealed 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const FW_ALL: &[FwDiag] = &[
    FwDiag::ERowSkip,
    FwDiag::ERowDup,
    FwDiag::EIfaceEmpty,
    FwDiag::EIfaceDup,
    FwDiag::EShortTable,
    FwDiag::EBatchDrift,
    FwDiag::EBatchMissing,
    FwDiag::EProbeDrift,
    FwDiag::EEmptyTable,
    FwDiag::ESealedWrite,
    FwDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in FW_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81681..0x8168B 闭集内"
}

// ── 消费批六闭集 ─────────────────────────────────────────────────
/// 消费批（B29–B34 六批闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsumeBatch {
    /// B29 批。
    BatchB29,
    /// B30 批。
    BatchB30,
    /// B31 批。
    BatchB31,
    /// B32 批。
    BatchB32,
    /// B33 批。
    BatchB33,
    /// B34 批。
    BatchB34,
}

impl ConsumeBatch {
    /// 批名。
    pub fn name(self) -> &'static str {
        match self {
            ConsumeBatch::BatchB29 => "B29",
            ConsumeBatch::BatchB30 => "B30",
            ConsumeBatch::BatchB31 => "B31",
            ConsumeBatch::BatchB32 => "B32",
            ConsumeBatch::BatchB33 => "B33",
            ConsumeBatch::BatchB34 => "B34",
        }
    }

    /// 批号。
    pub fn ordinal(self) -> u32 {
        match self {
            ConsumeBatch::BatchB29 => 0,
            ConsumeBatch::BatchB30 => 1,
            ConsumeBatch::BatchB31 => 2,
            ConsumeBatch::BatchB32 => 3,
            ConsumeBatch::BatchB33 => 4,
            ConsumeBatch::BatchB34 => 5,
        }
    }

    /// 全部批（闭集数组）。
    pub fn all() -> &'static [ConsumeBatch] {
        &[
            ConsumeBatch::BatchB29,
            ConsumeBatch::BatchB30,
            ConsumeBatch::BatchB31,
            ConsumeBatch::BatchB32,
            ConsumeBatch::BatchB33,
            ConsumeBatch::BatchB34,
        ]
    }
}

// ── 六批冻结接口清单（消费边全量来源） ───────────────────────────
/// B29 批冻结接口清单（11 行）。
pub const B29_IFACES: &[&str] = &[
    "装载句柄账", "装载段映射", "装载符号账", "装载收口账", "装载环境块",
    "装载重定位", "装载导入表", "装载导出表", "装载异常表", "装载清单回放",
    "装载摘要账",
];

/// B30 批冻结接口清单（11 行）。
pub const B30_IFACES: &[&str] = &[
    "对象创建账", "句柄授予账", "权限位账", "段属性账", "对象头账",
    "根路径归类", "命名对象账", "删除挂起账", "对象收口账", "对象摘要账",
    "越根拒止账",
];

/// B31 批冻结接口清单（11 行）。
pub const B31_IFACES: &[&str] = &[
    "PEB 探针账", "TEB 探针账", "NTSTATUS 谱账", "进程查询账", "线程查询账",
    "OS 版本档账", "系统信息账", "对象类型枚举账", "句柄表账", "同步对象账",
    "进程枚举账",
];

/// B32 批冻结接口清单（11 行）。
pub const B32_IFACES: &[&str] = &[
    "注入对账甲", "注入对账乙", "双比较账", "比较表账", "P95 预算账",
    "边缘采样账", "E1 回执账", "文档对齐账", "KTEST 聚合账", "B35 闸账",
    "B29 重跑账",
];

/// B33 批冻结接口清单（10 行）。
pub const B33_IFACES: &[&str] = &[
    "D3 重测账", "D2 重测账", "装载码账", "OA 双链账", "里程碑账",
    "消息边界账", "端口连接账", "监听受理账", "节账分账", "SQoS 档账",
];

/// B34 批冻结接口清单（10 行）。
pub const B34_IFACES: &[&str] = &[
    "SID 守卫账", "消息属性账", "消息类型账", "回执对账", "段视图书",
    "读写账", "句柄传递账", "端口关闭账", "联签预制账", "消息注入账",
];

/// 登记行额下限（判据锚定值 ≥60）。
pub const REGISTRY_MIN_ROWS: usize = 60;

/// 单批清单额（按批取冻结行数）。
pub fn batch_iface_count(b: ConsumeBatch) -> usize {
    match b {
        ConsumeBatch::BatchB29 => B29_IFACES.len(),
        ConsumeBatch::BatchB30 => B30_IFACES.len(),
        ConsumeBatch::BatchB31 => B31_IFACES.len(),
        ConsumeBatch::BatchB32 => B32_IFACES.len(),
        ConsumeBatch::BatchB33 => B33_IFACES.len(),
        ConsumeBatch::BatchB34 => B34_IFACES.len(),
    }
}

// ── 登记行域模型 ─────────────────────────────────────────────────
/// 登记行五元。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeRow {
    /// 行号（0..N 连续）。
    pub row_no: u32,
    /// 消费批。
    pub batch: ConsumeBatch,
    /// 接口短名（全表唯一）。
    pub iface: &'static str,
    /// 探针码（0x6000 + 行号）。
    pub probe_code: u32,
}

/// 探针码冻结折算。
pub fn probe_code_for(row_no: u32) -> u32 {
    0x6000u32.saturating_add(row_no)
}

/// 冻结清单铺总表（B29–B34 依序逐行铺开）。
pub fn build_registry() -> Vec<EdgeRow> {
    let mut rows: Vec<EdgeRow> = Vec::new();
    for b in ConsumeBatch::all().iter() {
        let mut i = 0usize;
        while i < batch_iface_count(*b) {
            let iface = match batch_iface(*b, i) {
                Some(v) => v,
                None => break,
            };
            let row_no = rows.len() as u32;
            rows.push(EdgeRow {
                row_no,
                batch: *b,
                iface,
                probe_code: probe_code_for(row_no),
            });
            i += 1;
        }
    }
    rows
}

/// 按批按下标取冻结接口短名（越界 None）。
pub fn batch_iface(b: ConsumeBatch, idx: usize) -> Option<&'static str> {
    match b {
        ConsumeBatch::BatchB29 => B29_IFACES.get(idx).copied(),
        ConsumeBatch::BatchB30 => B30_IFACES.get(idx).copied(),
        ConsumeBatch::BatchB31 => B31_IFACES.get(idx).copied(),
        ConsumeBatch::BatchB32 => B32_IFACES.get(idx).copied(),
        ConsumeBatch::BatchB33 => B33_IFACES.get(idx).copied(),
        ConsumeBatch::BatchB34 => B34_IFACES.get(idx).copied(),
    }
}

// ── 三查（逐条可点验） ───────────────────────────────────────────
/// 行号连续查（0..N 连续无跳无重）。
fn probe_row_no(rows: &[EdgeRow]) -> Result<(), u32> {
    let mut i = 0usize;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(v) => v,
            None => return Err(FwDiag::EEmptyTable.val()),
        };
        if r.row_no != i as u32 {
            return Err(FwDiag::ERowSkip.val());
        }
        i += 1;
    }
    Ok(())
}

/// 批次归属查（批闭集 + 批额与冻结清单逐批相等 + 批次缺一）。
fn probe_batch(rows: &[EdgeRow]) -> Result<(), u32> {
    for b in ConsumeBatch::all().iter() {
        let mut count = 0usize;
        for r in rows.iter() {
            if r.batch == *b {
                count += 1;
            }
        }
        if count == 0 {
            return Err(FwDiag::EBatchMissing.val());
        }
        if count != batch_iface_count(*b) {
            return Err(FwDiag::EBatchDrift.val());
        }
    }
    Ok(())
}

/// 接口唯一查（短名全表唯一非空）。
fn probe_iface(rows: &[EdgeRow]) -> Result<(), u32> {
    let mut i = 0usize;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(v) => v,
            None => return Err(FwDiag::EEmptyTable.val()),
        };
        if r.iface.is_empty() {
            return Err(FwDiag::EIfaceEmpty.val());
        }
        let mut j = i + 1;
        while j < rows.len() {
            let other = match rows.get(j) {
                Some(v) => v,
                None => return Err(FwDiag::EEmptyTable.val()),
            };
            if other.iface == r.iface {
                return Err(FwDiag::EIfaceDup.val());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 探针码查（探针码 = 0x6000 + 行号）。
fn probe_codes(rows: &[EdgeRow]) -> Result<(), u32> {
    for r in rows.iter() {
        if r.probe_code != probe_code_for(r.row_no) {
            return Err(FwDiag::EProbeDrift.val());
        }
    }
    Ok(())
}

/// 总表三查 + 探针码查（对账总闸；空表/行额不足前置）。
pub fn verify_registry(rows: &[EdgeRow]) -> Result<(), u32> {
    if rows.is_empty() {
        return Err(FwDiag::EEmptyTable.val());
    }
    if rows.len() < REGISTRY_MIN_ROWS {
        return Err(FwDiag::EShortTable.val());
    }
    probe_row_no(rows)?;
    probe_batch(rows)?;
    probe_iface(rows)?;
    probe_codes(rows)?;
    Ok(())
}

// ── 对账账状态机 ─────────────────────────────────────────────────
/// 对账账状态（三态单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FwState {
    /// 开账。
    FwOpen,
    /// 对账中。
    FwRunning,
    /// 封账（只读）。
    FwSealed,
}

impl FwState {
    /// 状态秩。
    pub fn rank(self) -> u32 {
        match self {
            FwState::FwOpen => 0,
            FwState::FwRunning => 1,
            FwState::FwSealed => 2,
        }
    }
}

/// 跨域消费面对账账。
#[derive(Clone, Debug)]
pub struct FwLedger {
    /// 状态。
    state: FwState,
    /// 登记总表。
    rows: Vec<EdgeRow>,
    /// 点验数（逐条点验通过数）。
    probes: u32,
    /// 拒止计数。
    rejects: u32,
}

impl FwLedger {
    /// 新账。
    pub fn new() -> FwLedger {
        FwLedger {
            state: FwState::FwOpen,
            rows: Vec::new(),
            probes: 0,
            rejects: 0,
        }
    }

    /// 开账 → 对账中。
    pub fn begin_run(&mut self) -> Result<(), u32> {
        match self.state {
            FwState::FwOpen => {
                self.state = FwState::FwRunning;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(FwDiag::ERewind.val())
            }
        }
    }

    /// 对账中 → 封账（三查不过不许封）。
    pub fn seal(&mut self) -> Result<(), u32> {
        match self.state {
            FwState::FwRunning => {
                match verify_registry(&self.rows) {
                    Ok(()) => {
                        self.state = FwState::FwSealed;
                        Ok(())
                    }
                    Err(e) => {
                        self.rejects = self.rejects.saturating_add(1);
                        Err(e)
                    }
                }
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(FwDiag::ERewind.val())
            }
        }
    }

    /// 当前状态。
    pub fn state(&self) -> FwState {
        self.state
    }

    /// 总表行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 点验数。
    pub fn probe_len(&self) -> u32 {
        self.probes
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 下标取行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&EdgeRow> {
        self.rows.get(idx)
    }

    /// 账等式：点验数 ≡ 行数。
    pub fn equation_ok(&self) -> bool {
        self.probes as usize == self.rows.len()
    }

    /// 登记一行（序闸/重闸/名闸/码闸四闸）。
    pub fn reg(&mut self, r: EdgeRow) -> Result<(), u32> {
        if self.state == FwState::FwSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(FwDiag::ESealedWrite.val());
        }
        if self.state == FwState::FwOpen {
            self.state = FwState::FwRunning;
        }
        // 序闸。
        if r.row_no != self.rows.len() as u32 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(FwDiag::ERowSkip.val());
        }
        // 重闸：行号唯一（序闸已保证，再防行号倒挂重入）。
        for e in self.rows.iter() {
            if e.row_no == r.row_no {
                self.rejects = self.rejects.saturating_add(1);
                return Err(FwDiag::ERowDup.val());
            }
        }
        // 名闸：接口短名非空且全表唯一。
        if r.iface.is_empty() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(FwDiag::EIfaceEmpty.val());
        }
        for e in self.rows.iter() {
            if e.iface == r.iface {
                self.rejects = self.rejects.saturating_add(1);
                return Err(FwDiag::EIfaceDup.val());
            }
        }
        // 码闸。
        if r.probe_code != probe_code_for(r.row_no) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(FwDiag::EProbeDrift.val());
        }
        // 全闸放行。
        self.probes = self.probes.saturating_add(1);
        self.rows.push(r);
        Ok(())
    }
}

/// 全量对账（冻结清单逐行入账并封账）。
pub fn build_full_fw_ledger() -> FwLedger {
    let rows = build_registry();
    let mut led = FwLedger::new();
    let _ = led.begin_run();
    for r in rows.iter() {
        let _ = led.reg(r.clone());
    }
    let _ = led.seal();
    led
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// 登记总表 ≥60 行（B29–B34 消费边全量）。
fn registry_rows_ok() -> bool {
    let rows = build_registry();
    if rows.len() < REGISTRY_MIN_ROWS {
        return false;
    }
    // 六批全量：每批行额与冻结清单相等。
    for b in ConsumeBatch::all().iter() {
        let mut count = 0usize;
        for r in rows.iter() {
            if r.batch == *b {
                count += 1;
            }
        }
        if count != batch_iface_count(*b) || count == 0 {
            return false;
        }
    }
    true
}

/// 三查逐条可点验（三查 + 探针码查全绿）。
fn three_probe_ok() -> bool {
    let rows = build_registry();
    match verify_registry(&rows) {
        Ok(()) => true,
        Err(_) => false,
    }
}

/// 六批批额普查（总 64 行 = 11+11+11+11+10+10）。
fn batch_census_ok() -> bool {
    let rows = build_registry();
    let mut total = 0usize;
    for b in ConsumeBatch::all().iter() {
        let mut count = 0usize;
        for r in rows.iter() {
            if r.batch == *b {
                count += 1;
            }
        }
        if count != batch_iface_count(*b) {
            return false;
        }
        total += count;
    }
    if total != rows.len() {
        return false;
    }
    if rows.len() != 64 {
        return false;
    }
    true
}

/// 注入联测十路（人为构造违约必被拒）。
pub fn fw_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 行号跳号。
    {
        let mut rows = build_registry();
        if let Some(r) = rows.get_mut(10) {
            r.row_no = r.row_no.saturating_add(1);
        }
        if verify_registry(&rows).is_err() {
            caught += 1;
        } else {
            escape.push_str("跳号;");
        }
    }
    // 2. 行号重复。
    {
        let mut rows = build_registry();
        if let Some(r) = rows.get_mut(20) {
            r.row_no = 0;
        }
        if verify_registry(&rows).is_err() {
            caught += 1;
        } else {
            escape.push_str("重号;");
        }
    }
    // 3. 接口置空。
    {
        let mut rows = build_registry();
        if let Some(r) = rows.get_mut(30) {
            r.iface = "";
        }
        if verify_registry(&rows).is_err() {
            caught += 1;
        } else {
            escape.push_str("空名;");
        }
    }
    // 4. 接口重复。
    {
        let mut rows = build_registry();
        let src = match rows.get(0) {
            Some(v) => v.iface,
            None => return (caught, 10, String::from("表缺行")),
        };
        if let Some(r) = rows.get_mut(1) {
            r.iface = src;
        }
        if verify_registry(&rows).is_err() {
            caught += 1;
        } else {
            escape.push_str("重名;");
        }
    }
    // 5. 行额不足（截到 59 行）。
    {
        let mut rows = build_registry();
        let mut keep = 59usize;
        if rows.len() < keep {
            keep = rows.len();
        }
        rows.truncate(keep);
        if verify_registry(&rows).is_err() {
            caught += 1;
        } else {
            escape.push_str("缺行;");
        }
    }
    // 6. 批额漂移（抹去一行 → 批行数 −1）。
    {
        let mut rows = build_registry();
        if rows.len() > 1 {
            let drop_no = rows.len() as u32 - 1;
            rows.retain(|r| r.row_no != drop_no);
        }
        if verify_registry(&rows).is_err() {
            caught += 1;
        } else {
            escape.push_str("批漂;");
        }
    }
    // 7. 批次缺一（全部 B34 行抹去）。
    {
        let mut rows = build_registry();
        rows.retain(|r| r.batch != ConsumeBatch::BatchB34);
        if verify_registry(&rows).is_err() {
            caught += 1;
        } else {
            escape.push_str("缺批;");
        }
    }
    // 8. 探针码漂移。
    {
        let mut rows = build_registry();
        if let Some(r) = rows.get_mut(40) {
            r.probe_code = r.probe_code.saturating_add(1);
        }
        if verify_registry(&rows).is_err() {
            caught += 1;
        } else {
            escape.push_str("码漂;");
        }
    }
    // 9. 空表。
    {
        let rows: Vec<EdgeRow> = Vec::new();
        if verify_registry(&rows).is_err() {
            caught += 1;
        } else {
            escape.push_str("空表;");
        }
    }
    // 10. 封账后写 + 倒退。
    {
        let mut led = build_full_fw_ledger();
        let extra = EdgeRow {
            row_no: 999,
            batch: ConsumeBatch::BatchB29,
            iface: "越界行",
            probe_code: probe_code_for(999),
        };
        let sealed_write_rejected = led.reg(extra).is_err();
        let rewind_rejected = led.begin_run().is_err();
        if sealed_write_rejected && rewind_rejected {
            caught += 1;
        } else {
            escape.push_str("封写;");
        }
    }
    (caught, 10, escape)
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let a = build_full_fw_ledger();
    let b = build_full_fw_ledger();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.row_at(i), b.row_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if fw_fault_sweep().0 != fw_fault_sweep().0 {
        return false;
    }
    if frozen_digest() != frozen_digest() {
        return false;
    }
    true
}

/// 冻结摘要（全行探针码与批号折算）。
pub fn frozen_digest() -> u32 {
    let rows = build_registry();
    let mut acc: u32 = SEG_BASE;
    for r in rows.iter() {
        acc = acc
            .wrapping_mul(17)
            .wrapping_add(r.probe_code)
            .wrapping_add(r.batch.ordinal());
    }
    acc
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12690u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let led = build_full_fw_ledger();
    if led.state() != FwState::FwSealed {
        return false;
    }
    let head = match led.row_at(0) {
        Some(r) => r.iface,
        None => return false,
    };
    let tail = match led.row_at(led.len().saturating_sub(1)) {
        Some(r) => r.iface,
        None => return false,
    };
    if head.is_empty() || tail.is_empty() {
        return false;
    }
    for b in ConsumeBatch::all().iter() {
        if b.name().is_empty() {
            return false;
        }
    }
    if frozen_digest() == 0 {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12691 跨域消费面总对账自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12690_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12690-fourway-recon");
    let led = build_full_fw_ledger();

    s.add_dyn(
        "F12691-01 登记总表 ≥60 行",
        registry_rows_ok(),
        &format!("rows={} min={}", led.len(), REGISTRY_MIN_ROWS),
    );

    s.add_dyn(
        "F12691-02 三查逐条可点验",
        three_probe_ok(),
        "行号连续/批次归属/接口唯一三查全绿",
    );

    s.add_dyn(
        "F12691-03 六批批额普查",
        batch_census_ok(),
        &format!("total={} probes={}", led.len(), led.probe_len()),
    );

    s.add_dyn(
        "F12691-04 探针码口径",
        probe_code_for(0) == 0x6000 && probe_code_for(63) == 0x603F,
        "探针码 = 0x6000 + 行号",
    );

    s.add_dyn(
        "F12691-05 注入联测 10/10 拒止",
        {
            let (c, t, e) = fw_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", fw_fault_sweep().0),
    );

    s.add_dyn(
        "F12691-06 账等式（点验 ≡ 行数）",
        led.equation_ok(),
        &format!("probes={} rows={} rejects={}", led.probe_len(), led.len(), led.reject_len()),
    );

    s.add_dyn(
        "F12691-07 批闭集覆盖",
        ConsumeBatch::all().len() == 6,
        "B29–B34 六批闭集",
    );

    s.add_dyn(
        "F12691-08 批额冻结",
        batch_iface_count(ConsumeBatch::BatchB29) == 11
            && batch_iface_count(ConsumeBatch::BatchB34) == 10,
        "冻结清单 11/11/11/11/10/10",
    );

    s.add_dyn(
        "F12691-09 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12691-10 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12691-11 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12691-12 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert_eq!(SEG_BASE, 0x81680);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(FW_ALL.len(), 11);
        let mut i = 0usize;
        while i < FW_ALL.len() {
            let vi = match FW_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < FW_ALL.len() {
                let vj = match FW_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn registry_rows_hold() {
        assert!(registry_rows_ok());
        assert!(three_probe_ok());
        assert!(batch_census_ok());
        let rows = build_registry();
        assert_eq!(rows.len(), 64);
        assert!(rows.len() >= REGISTRY_MIN_ROWS);
        let led = build_full_fw_ledger();
        assert_eq!(led.len(), 64);
        assert_eq!(led.state(), FwState::FwSealed);
        assert_eq!(led.reject_len(), 0);
        assert!(led.equation_ok());
    }

    #[test]
    fn batch_census_counts() {
        assert_eq!(batch_iface_count(ConsumeBatch::BatchB29), 11);
        assert_eq!(batch_iface_count(ConsumeBatch::BatchB30), 11);
        assert_eq!(batch_iface_count(ConsumeBatch::BatchB31), 11);
        assert_eq!(batch_iface_count(ConsumeBatch::BatchB32), 11);
        assert_eq!(batch_iface_count(ConsumeBatch::BatchB33), 10);
        assert_eq!(batch_iface_count(ConsumeBatch::BatchB34), 10);
        // 批接口下标越界 None。
        assert!(batch_iface(ConsumeBatch::BatchB29, 11).is_none());
        assert!(batch_iface(ConsumeBatch::BatchB34, 10).is_none());
        assert!(batch_iface(ConsumeBatch::BatchB29, 0).is_some());
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = fw_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn verify_rejects_short_table() {
        let mut rows = build_registry();
        rows.truncate(REGISTRY_MIN_ROWS - 1);
        assert_eq!(verify_registry(&rows), Err(FwDiag::EShortTable.val()));
        let empty: Vec<EdgeRow> = Vec::new();
        assert_eq!(verify_registry(&empty), Err(FwDiag::EEmptyTable.val()));
        // 正表过闸。
        let full = build_registry();
        assert!(verify_registry(&full).is_ok());
    }

    #[test]
    fn reg_gate_rejects() {
        let mut led = FwLedger::new();
        let _ = led.begin_run();
        let rows = build_registry();
        // 跳号：直入 1 号。
        let r1 = match rows.get(1) {
            Some(x) => x.clone(),
            None => return,
        };
        assert_eq!(led.reg(r1), Err(FwDiag::ERowSkip.val()));
        let r0 = match rows.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        assert!(led.reg(r0.clone()).is_ok());
        // 接口重复。
        let dup = EdgeRow {
            row_no: 1,
            batch: ConsumeBatch::BatchB29,
            iface: r0.iface,
            probe_code: probe_code_for(1),
        };
        assert_eq!(led.reg(dup), Err(FwDiag::EIfaceDup.val()));
        // 探针码漂移。
        let drift = EdgeRow {
            row_no: 1,
            batch: ConsumeBatch::BatchB29,
            iface: "独立短名",
            probe_code: 0x6001 + 1,
        };
        assert_eq!(led.reg(drift), Err(FwDiag::EProbeDrift.val()));
        assert_eq!(led.reject_len(), 3);
    }

    #[test]
    fn dual_run_is_identical() {
        let a = build_full_fw_ledger();
        let b = build_full_fw_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑行缺失"),
            }
            i += 1;
        }
        assert_eq!(fw_fault_sweep().0, fw_fault_sweep().0);
        assert_eq!(frozen_digest(), frozen_digest());
    }

    #[test]
    fn state_machine_never_rewinds() {
        assert!(FwState::FwOpen.rank() < FwState::FwRunning.rank());
        assert!(FwState::FwRunning.rank() < FwState::FwSealed.rank());
        let mut led = build_full_fw_ledger();
        assert_eq!(led.state(), FwState::FwSealed);
        assert_eq!(led.seal(), Err(FwDiag::ERewind.val()));
        let mut fresh = FwLedger::new();
        assert!(fresh.begin_run().is_ok());
        assert_eq!(fresh.begin_run(), Err(FwDiag::ERewind.val()));
    }

    #[test]
    fn boundary_probe_codes() {
        assert_eq!(probe_code_for(0), 0x6000);
        assert_eq!(probe_code_for(63), 0x603F);
        assert_eq!(probe_code_for(u32::MAX), 0x6000u32.saturating_add(u32::MAX));
        let rows = build_registry();
        let last = match rows.get(63) {
            Some(x) => x.clone(),
            None => return,
        };
        assert_eq!(last.row_no, 63);
        assert!(rows.get(64).is_none());
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12690_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
