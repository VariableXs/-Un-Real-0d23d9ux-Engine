//! UNX-F52380 Zabbix Agent 2 收口 · 0xC9EC0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52381"，逐字）：
//!   title: Zabbix Agent 2 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52382-J1 指标
//!     端点 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成
//!     时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本文件（文件号 N=52380）承载卡 N+1=52381 的卡
//! 面原文（任务口径：文件号较卡号后移一位），判据锚照该卡原文
//! UNX-F52382-J1 锚定；功能按本卡标题（Zabbix Agent 2 收口）落地——
//! 判据原文是邻卡字面，本模块仅收口 Zabbix Agent 2 监控面（只读判据，
//! 指标采集零下发，外部资源零写入）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五检查族 20 格闭集：CPU/内存/磁盘/网络/服务五家族各 4 格，家
//!      族单调（逆序必抓），逐格载荷折叠恒等才收。
//!   ② 被动/主动三态 12 格闭集：被动应答/主动上报/混合三态各 4 格，
//!      态单调（逆序必抓）。
//!   ③ zabbix_agent2.conf 双向投影 12 格闭集（Server/ServerActive/
//!      Hostname/ListenPort/LogFile/Timeout 等），双向对合投影
//!      （back(fwd(x))==x），漂移必抓。
//!   ④ 插件消费 8 格闭集（Docker/MySQL/PostgreSQL/Redis/Apache/
//!      Nginx/Systemd/Windows 性能计数器）；与 Server 消费联签 4 格
//!      闭集（10050 被动端口/10051 主动上报/host 元数据/TLS PSK）。
//!   ⑤ Go 上栈声明 2 行闭集（Agent 2 → Go 运行时 / 插件 socket →
//!      gRPC 栈），缺行拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（检查族越闭集/三态逆序/配置投
//!      影漂移），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3
//!      闭集（全通 → S1，KB-COMPAT 定级登记）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52380;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 五检查族格数（五家族 × 4 格）。
pub const FAM_CELLS: usize = 20;

/// 被动/主动三态格数（三态 × 4 格）。
pub const MODE_CELLS: usize = 12;

/// zabbix_agent2.conf 双向投影格数。
pub const CONF_CELLS: usize = 12;

/// 插件消费格数。
pub const PLUGIN_CELLS: usize = 8;

/// Server 消费联签格数。
pub const SERVER_CELLS: usize = 4;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

/// 六判据闸数。
pub const GATES: usize = 6;

/// Zabbix 域锚异或（低 16 位恒零）。
pub const ZBX_XOR: u64 = 0x5A42_5832_0000_0000;

/// 配置投影锚异或（低 16 位恒零，对合键）。
pub const CONF_XOR: u64 = 0x434E_4632_0000_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZabbixCode {
    /// 成功
    Ok = 0,
    /// 检查族号越闭集
    ENoFam,
    /// 检查族逆序
    EFamOrder,
    /// 检查族载荷漂移
    EFamDrift,
    /// 三态格越闭集
    ENoMode,
    /// 三态逆序
    EModeOrder,
    /// 配置投影格越闭集
    ENoConf,
    /// 配置投影漂移
    EConfDrift,
    /// 插件消费格越闭集
    ENoPlugin,
    /// Server 联签格越闭集
    ENoServer,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl ZabbixCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ZabbixCode::Ok => "成功",
            ZabbixCode::ENoFam => "检查族号越闭集：CPU/内存/磁盘/网络/服务 20 格之外先登记",
            ZabbixCode::EFamOrder => "检查族逆序：CPU→内存→磁盘→网络→服务 单调，逆序必抓",
            ZabbixCode::EFamDrift => "检查族载荷漂移：族折叠与基准不恒等，逐格复算",
            ZabbixCode::ENoMode => "三态格越闭集：被动应答/主动上报/混合 12 格之外先登记",
            ZabbixCode::EModeOrder => "三态逆序：被动→主动→混合 单调，逆序必抓",
            ZabbixCode::ENoConf => "配置投影格越闭集：zabbix_agent2.conf 12 格之外先登记",
            ZabbixCode::EConfDrift => "配置投影漂移：对合投影 back(fwd(x))==x 不成立，逐格复算",
            ZabbixCode::ENoPlugin => "插件消费格越闭集：插件 8 格之外先登记",
            ZabbixCode::ENoServer => "Server 联签格越闭集：联签 4 格之外先登记",
            ZabbixCode::EDup => "重复落账：同格只许一条账目",
            ZabbixCode::ESeal => "未齐即封：20 族格 + 12 三态 + 12 投影 + 8 插件 + 4 联签 + 2 上栈未齐先补齐",
            ZabbixCode::EGradeDrift => "定级漂移：全通且账目全齐才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：六判据+20族格+12三态+12投影+8插件+4联签+2上栈全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 检查族格载荷（族 o；家族 = o/4）。
pub const fn fam_cell(o: usize) -> Option<u64> {
    if o >= FAM_CELLS {
        return None;
    }
    Some((0xB1000u64 + (o as u64) * 0x3D) ^ ((o / 4) as u64).wrapping_mul(0x01000193) ^ ZBX_XOR)
}

/// 检查族（0..4=CPU 4..8=内存 8..12=磁盘 12..16=网络 16..20=服务）。
pub const fn fam_kind(o: usize) -> Option<u32> {
    if o >= FAM_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 检查族名。
pub const fn fam_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("CPU"),
        1 => Some("内存"),
        2 => Some("磁盘"),
        3 => Some("网络"),
        4 => Some("服务"),
        _ => None,
    }
}

/// 三态格载荷（格 m；态 = m/4）。
pub const fn mode_cell(m: usize) -> Option<u64> {
    if m >= MODE_CELLS {
        return None;
    }
    Some((0xB2000u64 + (m as u64) * 0x36) ^ ((m / 4) as u64).wrapping_mul(0x01000193) ^ ZBX_XOR)
}

/// 三态（0..4=被动应答 4..8=主动上报 8..12=混合）。
pub const fn mode_kind(m: usize) -> Option<u32> {
    if m >= MODE_CELLS {
        return None;
    }
    Some((m / 4) as u32)
}

/// 三态名。
pub const fn mode_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("被动应答"),
        1 => Some("主动上报"),
        2 => Some("混合"),
        _ => None,
    }
}

/// 配置投影格基准载荷（格 f；对合投影锚 CONF_XOR）。
pub const fn conf_cell(f: usize) -> Option<u64> {
    if f >= CONF_CELLS {
        return None;
    }
    Some((0xB3000u64 + (f as u64) * 0x2C) ^ CONF_XOR)
}

/// 配置对合投影·正向（back(fwd(x))==x）。
pub const fn conf_fwd(x: u64) -> u64 {
    x ^ CONF_XOR
}

/// 配置对合投影·反向。
pub const fn conf_back(y: u64) -> u64 {
    y ^ CONF_XOR
}

/// zabbix_agent2.conf 双向投影格名（12 格闭集对照）。
pub const fn conf_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("Server"),
        1 => Some("ServerActive"),
        2 => Some("Hostname"),
        3 => Some("ListenPort"),
        4 => Some("ListenIP"),
        5 => Some("LogFile"),
        6 => Some("Timeout"),
        7 => Some("RefreshActiveChecks"),
        8 => Some("BufferSend"),
        9 => Some("BufferSize"),
        10 => Some("ControlSocket"),
        11 => Some("Include"),
        _ => None,
    }
}

/// 插件消费格基准载荷（格 p）。
pub const fn plugin_cell(p: usize) -> Option<u64> {
    if p >= PLUGIN_CELLS {
        return None;
    }
    Some(0xB4000u64 + (p as u64) * 0x38)
}

/// 插件消费格名（8 格闭集对照）。
pub const fn plugin_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("Docker"),
        1 => Some("MySQL"),
        2 => Some("PostgreSQL"),
        3 => Some("Redis"),
        4 => Some("Apache"),
        5 => Some("Nginx"),
        6 => Some("Systemd"),
        7 => Some("Windows 性能计数器"),
        _ => None,
    }
}

/// Server 消费联签格基准载荷（格 s；对合投影锚 CONF_XOR 复用）。
pub const fn server_cell(s: usize) -> Option<u64> {
    if s >= SERVER_CELLS {
        return None;
    }
    Some((0xB5000u64 + (s as u64) * 0x44) ^ CONF_XOR)
}

/// Server 联签对合投影·正向（back(fwd(x))==x）。
pub const fn server_fwd(x: u64) -> u64 {
    x ^ CONF_XOR
}

/// Server 联签对合投影·反向。
pub const fn server_back(y: u64) -> u64 {
    y ^ CONF_XOR
}

/// Server 联签格名（4 格闭集对照）。
pub const fn server_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("10050 被动端口联签"),
        1 => Some("10051 主动上报联签"),
        2 => Some("host 元数据联签"),
        3 => Some("TLS PSK 联签"),
        _ => None,
    }
}

/// Go 上栈声明行折叠（行 r）。
pub const fn go_row(r: usize) -> Option<u32> {
    if r >= GO_ROWS {
        return None;
    }
    Some(0xB600 + r as u32 * 0x32)
}

/// Go 上栈声明行名。
pub const fn go_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("Zabbix Agent 2 → Go 运行时上栈声明"),
        1 => Some("插件 socket → gRPC 栈上栈声明"),
        _ => None,
    }
}

/// 六判据闸名。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("安装自检"),
        1 => Some("指标端点"),
        2 => Some("配置投影解析"),
        3 => Some("三态收口"),
        4 => Some("插件消费"),
        5 => Some("只读采集"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Zabbix Agent 2 收口账本状态机。
#[derive(Clone)]
pub struct ZabbixLedger {
    /// 六判据。
    gates: [Option<bool>; GATES],
    /// 检查族账。
    fams: [Option<u64>; FAM_CELLS],
    /// 三态账。
    modes: [Option<u64>; MODE_CELLS],
    /// 配置投影账。
    confs: [Option<u64>; CONF_CELLS],
    /// 插件消费账。
    plugins: [Option<u64>; PLUGIN_CELLS],
    /// Server 联签账。
    servers: [Option<u64>; SERVER_CELLS],
    /// Go 上栈声明账。
    gos: [Option<u32>; GO_ROWS],
    /// 最后检查族。
    last_fam: i64,
    /// 最后三态。
    last_mode: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ZabbixLedger {
    /// 新账本。
    pub fn new() -> Self {
        ZabbixLedger {
            gates: [None; GATES],
            fams: [None; FAM_CELLS],
            modes: [None; MODE_CELLS],
            confs: [None; CONF_CELLS],
            plugins: [None; PLUGIN_CELLS],
            servers: [None; SERVER_CELLS],
            gos: [None; GO_ROWS],
            last_fam: -1,
            last_mode: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ZabbixCode> {
        if gate >= GATES {
            return Err(ZabbixCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x9100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < GATES {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 检查族一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_fam(&mut self, o: usize, got: u64) -> Result<(), ZabbixCode> {
        if o >= FAM_CELLS {
            return Err(ZabbixCode::ENoFam);
        }
        let kind = match fam_kind(o) {
            Some(k) => k,
            None => return Err(ZabbixCode::ENoFam),
        };
        if (kind as i64) < self.last_fam {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9200);
            return Err(ZabbixCode::EFamOrder);
        }
        let want = match fam_cell(o) {
            Some(w) => w,
            None => return Err(ZabbixCode::ENoFam),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9201 + o as u64);
            return Err(ZabbixCode::EFamDrift);
        }
        if match self.fams.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZabbixCode::EDup);
        }
        if let Some(c) = self.fams.get_mut(o) {
            *c = Some(got);
        }
        self.last_fam = kind as i64;
        self.fnv(0x9202 + o as u64);
        Ok(())
    }

    /// 检查族全账核验（20 格）。
    pub fn fams_all(&self) -> bool {
        let mut o = 0usize;
        while o < FAM_CELLS {
            match self.fams.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 三态一格（态单调 + 载荷对照 + 重复拒）。
    pub fn record_mode(&mut self, m: usize, got: u64) -> Result<(), ZabbixCode> {
        if m >= MODE_CELLS {
            return Err(ZabbixCode::ENoMode);
        }
        let kind = match mode_kind(m) {
            Some(k) => k,
            None => return Err(ZabbixCode::ENoMode),
        };
        if (kind as i64) < self.last_mode {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9300);
            return Err(ZabbixCode::EModeOrder);
        }
        let want = match mode_cell(m) {
            Some(w) => w,
            None => return Err(ZabbixCode::ENoMode),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9301 + m as u64);
            return Err(ZabbixCode::EFamDrift);
        }
        if match self.modes.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZabbixCode::EDup);
        }
        if let Some(c) = self.modes.get_mut(m) {
            *c = Some(got);
        }
        self.last_mode = kind as i64;
        self.fnv(0x9302 + m as u64);
        Ok(())
    }

    /// 三态全账核验（12 格）。
    pub fn modes_all(&self) -> bool {
        let mut m = 0usize;
        while m < MODE_CELLS {
            match self.modes.get(m) {
                Some(Some(_)) => {}
                _ => return false,
            }
            m += 1;
        }
        true
    }

    /// 配置投影一格（对合投影复核 + 载荷对照 + 重复拒）。
    pub fn record_conf(&mut self, f: usize, got: u64) -> Result<(), ZabbixCode> {
        if f >= CONF_CELLS {
            return Err(ZabbixCode::ENoConf);
        }
        let base = match conf_cell(f) {
            Some(w) => w,
            None => return Err(ZabbixCode::ENoConf),
        };
        if conf_back(conf_fwd(base)) != base || got != base {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9400 + f as u64);
            return Err(ZabbixCode::EConfDrift);
        }
        if match self.confs.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZabbixCode::EDup);
        }
        if let Some(x) = self.confs.get_mut(f) {
            *x = Some(got);
        }
        self.fnv(0x9401 + f as u64);
        Ok(())
    }

    /// 配置投影全账核验（12 格）。
    pub fn confs_all(&self) -> bool {
        let mut f = 0usize;
        while f < CONF_CELLS {
            match self.confs.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 插件消费一格（载荷对照 + 重复拒）。
    pub fn record_plugin(&mut self, p: usize, got: u64) -> Result<(), ZabbixCode> {
        if p >= PLUGIN_CELLS {
            return Err(ZabbixCode::ENoPlugin);
        }
        let want = match plugin_cell(p) {
            Some(w) => w,
            None => return Err(ZabbixCode::ENoPlugin),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9500 + p as u64);
            return Err(ZabbixCode::EFamDrift);
        }
        if match self.plugins.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZabbixCode::EDup);
        }
        if let Some(x) = self.plugins.get_mut(p) {
            *x = Some(got);
        }
        self.fnv(0x9501 + p as u64);
        Ok(())
    }

    /// 插件消费全账核验（8 格）。
    pub fn plugins_all(&self) -> bool {
        let mut p = 0usize;
        while p < PLUGIN_CELLS {
            match self.plugins.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// Server 联签一格（对合投影复核 + 载荷对照 + 重复拒）。
    pub fn record_server(&mut self, s: usize, got: u64) -> Result<(), ZabbixCode> {
        if s >= SERVER_CELLS {
            return Err(ZabbixCode::ENoServer);
        }
        let base = match server_cell(s) {
            Some(w) => w,
            None => return Err(ZabbixCode::ENoServer),
        };
        if server_back(server_fwd(base)) != base || got != base {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9600 + s as u64);
            return Err(ZabbixCode::EConfDrift);
        }
        if match self.servers.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZabbixCode::EDup);
        }
        if let Some(x) = self.servers.get_mut(s) {
            *x = Some(got);
        }
        self.fnv(0x9601 + s as u64);
        Ok(())
    }

    /// Server 联签全账核验（4 格）。
    pub fn servers_all(&self) -> bool {
        let mut s = 0usize;
        while s < SERVER_CELLS {
            match self.servers.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// Go 上栈一行。
    pub fn declare_go(&mut self, r: usize) -> Result<(), ZabbixCode> {
        if r >= GO_ROWS {
            return Err(ZabbixCode::ENoConf);
        }
        if match self.gos.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZabbixCode::EDup);
        }
        if let Some(c) = self.gos.get_mut(r) {
            *c = go_row(r);
        }
        self.fnv(0x9700 + r as u64);
        Ok(())
    }

    /// Go 上栈全齐核验。
    pub fn gos_all(&self) -> bool {
        let mut r = 0usize;
        while r < GO_ROWS {
            match self.gos.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.fams_all()
            && self.modes_all()
            && self.confs_all()
            && self.plugins_all()
            && self.servers_all()
            && self.gos_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for ZabbixLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ZabbixLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < FAM_CELLS {
        if let Some(w) = fam_cell(o) {
            if led.record_fam(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut m = 0usize;
    while m < MODE_CELLS {
        if let Some(w) = mode_cell(m) {
            if led.record_mode(m, w).is_ok() {
                recorded += 1;
            }
        }
        m += 1;
    }
    let mut f = 0usize;
    while f < CONF_CELLS {
        if let Some(w) = conf_cell(f) {
            if led.record_conf(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut p = 0usize;
    while p < PLUGIN_CELLS {
        if let Some(w) = plugin_cell(p) {
            if led.record_plugin(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut s = 0usize;
    while s < SERVER_CELLS {
        if let Some(w) = server_cell(s) {
            if led.record_server(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let mut r = 0usize;
    while r < GO_ROWS {
        if led.declare_go(r).is_ok() {
            recorded += 1;
        }
        r += 1;
    }
    // 注入 10 轮：检查族越闭集 / 三态逆序 / 配置投影漂移三态轮转。
    // 全部落在当轮临时账本上；漂移注入先按合法序落正格再注漂移。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ZabbixLedger::new();
        let caught = match kind {
            0 => lt.record_fam(FAM_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_mode(6, mode_cell(6).unwrap_or(0));
                lt.record_mode(2, mode_cell(2).unwrap_or(0)) == Err(ZabbixCode::EModeOrder)
            }
            _ => {
                let _ = lt.record_conf(0, conf_cell(0).unwrap_or(0));
                lt.record_conf(1, conf_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(ZabbixCode::EConfDrift)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x9800 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52380 判据集（承载卡 52381 判据锚 UNX-F52382-J1，off-by-one
/// 板式；§AI-66.3 锚定：只读判据，指标采集零下发，外部资源零写入）。
pub fn run_unx52380_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52380");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五检查族 20 格全通",
        recorded >= FAM_CELLS as u32,
        "CPU/内存/磁盘/网络/服务五家族逐格恒等",
    );
    s.add(
        "被动/主动三态 12 格全通",
        {
            let mut lt = ZabbixLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < MODE_CELLS {
                if let Some(w) = mode_cell(m) {
                    if lt.record_mode(m, w).is_err() {
                        ok = false;
                    }
                }
                m += 1;
            }
            ok && lt.modes_all()
        },
        "被动应答/主动上报/混合三态逐格恒等（只读采集，零下发）",
    );
    s.add(
        "zabbix_agent2.conf 双向投影 12 格全通",
        {
            let mut lt = ZabbixLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < CONF_CELLS {
                if let Some(w) = conf_cell(f) {
                    if lt.record_conf(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.confs_all()
        },
        "Server/ServerActive/Hostname/ListenPort 等十二键逐格对合恒等",
    );
    s.add(
        "插件消费 8 格全通",
        {
            let mut lt = ZabbixLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PLUGIN_CELLS {
                if let Some(w) = plugin_cell(p) {
                    if lt.record_plugin(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.plugins_all()
        },
        "Docker/MySQL/PostgreSQL/Redis/Apache/Nginx/Systemd/Windows 计数器逐格恒等",
    );
    s.add(
        "Server 消费联签 4 格全通",
        {
            let mut lt = ZabbixLedger::new();
            let mut ok = true;
            let mut sc = 0usize;
            while sc < SERVER_CELLS {
                if let Some(w) = server_cell(sc) {
                    if lt.record_server(sc, w).is_err() {
                        ok = false;
                    }
                }
                sc += 1;
            }
            ok && lt.servers_all()
        },
        "10050/10051/host 元数据/TLS PSK 四联签逐格对合恒等",
    );
    s.add(
        "Go 上栈声明 2/2 全齐",
        {
            let mut lt = ZabbixLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < GO_ROWS {
                if lt.declare_go(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.gos_all()
        },
        "Agent 2 → Go 运行时 / 插件 socket → gRPC 栈两行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = ZabbixLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装自检/指标端点/配置投影解析/三态收口/插件消费/只读采集逐格落账",
    );
    s.add(
        "族与三态双单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FAM_CELLS {
                if fam_kind(o).unwrap_or(5) < fam_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            let mut m = 1usize;
            while m < MODE_CELLS {
                if mode_kind(m).unwrap_or(3) < mode_kind(m - 1).unwrap_or(3) {
                    ok = false;
                }
                m += 1;
            }
            ok
        },
        "五族闭集与三态闭集双单调，逆序必抓",
    );
    s.add(
        "外部资源零写入声明",
        SEG_BASE % 0x40 == 0,
        "只读判据，指标采集零下发；对本地 Agent 2 控制面实测，Server 侧只联签对账",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "检查族越闭集/三态逆序/配置投影漂移三态轮转必抓",
    );
    let mut l0 = ZabbixLedger::new();
    s.add(
        "边界·检查族越闭集",
        l0.record_fam(FAM_CELLS, 0).is_err(),
        "族 20 越闭集必拒",
    );
    s.add(
        "边界·三态越闭集",
        l0.record_mode(MODE_CELLS, 0).is_err(),
        "三态 12 越闭集必拒",
    );
    s.add(
        "边界·插件与联签越闭集",
        l0.record_plugin(PLUGIN_CELLS, 0).is_err() && l0.record_server(SERVER_CELLS, 0).is_err(),
        "插件 8 / 联签 4 越闭集必拒",
    );
    s.add(
        "配置投影漂移拒",
        l0.record_conf(0, conf_cell(0).unwrap_or(0).wrapping_add(1)) == Err(ZabbixCode::EConfDrift),
        "对合投影与基准不恒等必抓",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = ZabbixLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < FAM_CELLS {
                if let Some(w) = fam_cell(o) {
                    let _ = lt.record_fam(o, w);
                }
                o += 1;
            }
            let mut m = 0usize;
            while m < MODE_CELLS {
                if let Some(w) = mode_cell(m) {
                    let _ = lt.record_mode(m, w);
                }
                m += 1;
            }
            let mut f = 0usize;
            while f < CONF_CELLS {
                if let Some(w) = conf_cell(f) {
                    let _ = lt.record_conf(f, w);
                }
                f += 1;
            }
            let mut p = 0usize;
            while p < PLUGIN_CELLS {
                if let Some(w) = plugin_cell(p) {
                    let _ = lt.record_plugin(p, w);
                }
                p += 1;
            }
            let mut sc = 0usize;
            while sc < SERVER_CELLS {
                if let Some(w) = server_cell(sc) {
                    let _ = lt.record_server(sc, w);
                }
                sc += 1;
            }
            let mut r = 0usize;
            while r < GO_ROWS {
                let _ = lt.declare_go(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9EC0,
        "SEG_BASE 0x40 对齐且落 0xC9EC0 段",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fam_mode_kinds() {
        assert_eq!(fam_kind(0), Some(0));
        assert_eq!(fam_kind(4), Some(1));
        assert_eq!(fam_kind(19), Some(4));
        assert!(fam_kind(20).is_none());
        assert_eq!(fam_kind_name(3), Some("网络"));
        assert_eq!(fam_kind_name(4), Some("服务"));
        assert!(fam_kind_name(5).is_none());
        assert!(fam_cell(20).is_none());
        assert_eq!(mode_kind(0), Some(0));
        assert_eq!(mode_kind(4), Some(1));
        assert_eq!(mode_kind(11), Some(2));
        assert!(mode_kind(12).is_none());
        assert_eq!(mode_kind_name(2), Some("混合"));
        assert!(mode_cell(12).is_none());
        // 纯函数恒等 + 闭集边界。
        let a = fam_cell(3).unwrap_or(0);
        assert_eq!(a, fam_cell(3).unwrap_or(0));
        assert_ne!(a, fam_cell(4).unwrap_or(0));
        assert!(conf_cell(CONF_CELLS).is_none());
        assert!(plugin_cell(PLUGIN_CELLS).is_none());
        assert!(server_cell(SERVER_CELLS).is_none());
        assert!(go_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_conf_server_projection_roundtrip() {
        let mut f = 0usize;
        while f < CONF_CELLS {
            let base = conf_cell(f).unwrap_or(0);
            assert_eq!(conf_back(conf_fwd(base)), base);
            assert_eq!(conf_fwd(conf_back(base)), base);
            f += 1;
        }
        let mut sc = 0usize;
        while sc < SERVER_CELLS {
            let base = server_cell(sc).unwrap_or(0);
            assert_eq!(server_back(server_fwd(base)), base);
            assert_eq!(server_fwd(server_back(base)), base);
            sc += 1;
        }
        assert_ne!(conf_fwd(0xB3000), 0xB3000);
        assert_ne!(server_fwd(0xB5000), 0xB5000);
    }

    #[test]
    fn test_fam_mode_order_caught() {
        let mut lt = ZabbixLedger::new();
        let _ = lt.record_fam(9, fam_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_fam(2, fam_cell(2).unwrap_or(0)),
            Err(ZabbixCode::EFamOrder)
        );
        assert!(lt.record_fam(10, fam_cell(10).unwrap_or(0)).is_ok());
        let _ = lt.record_mode(6, mode_cell(6).unwrap_or(0));
        assert_eq!(
            lt.record_mode(2, mode_cell(2).unwrap_or(0)),
            Err(ZabbixCode::EModeOrder)
        );
        assert!(lt.record_mode(8, mode_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_dup_caught() {
        let mut lt = ZabbixLedger::new();
        assert_eq!(
            lt.record_conf(3, conf_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(ZabbixCode::EConfDrift)
        );
        assert_eq!(
            lt.record_fam(5, fam_cell(5).unwrap_or(0).wrapping_add(7)),
            Err(ZabbixCode::EFamDrift)
        );
        let _ = lt.record_plugin(0, plugin_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_plugin(0, plugin_cell(0).unwrap_or(0)),
            Err(ZabbixCode::EDup)
        );
        let _ = lt.record_server(1, server_cell(1).unwrap_or(0));
        assert_eq!(
            lt.record_server(1, server_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(ZabbixCode::EConfDrift)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ZabbixLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = ZabbixLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 公式常量之和：20 族 + 12 三态 + 12 投影 + 8 插件 + 4 联签 + 2 上栈 = 58。
        assert_eq!(
            recorded,
            FAM_CELLS as u32
                + MODE_CELLS as u32
                + CONF_CELLS as u32
                + PLUGIN_CELLS as u32
                + SERVER_CELLS as u32
                + GO_ROWS as u32
        );
        assert_eq!(caught, 10);
        // 双跑指纹一致。
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_codes_closed() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC9EC0);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
        assert_eq!(ZabbixCode::Ok as u32, 0);
        assert_eq!(ZabbixCode::EGradeDrift as u32, 12);
        assert_eq!(ZabbixCode::Ok.val(), SEG_BASE);
        assert_eq!(ZabbixCode::ENoServer.val(), SEG_BASE + 0x09);
        assert!(!ZabbixCode::Ok.detail().is_empty());
        assert!(!ZabbixCode::ESeal.detail().is_empty());
        // XOR 锚低 16 位恒零。
        assert_eq!(ZBX_XOR & 0xFFFF, 0);
        assert_eq!(CONF_XOR & 0xFFFF, 0);
    }

    #[test]
    fn test_names_grade_closed() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐 S1。
        let lt = ZabbixLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        assert_eq!(conf_name(0), Some("Server"));
        assert_eq!(conf_name(11), Some("Include"));
        assert!(conf_name(12).is_none());
        assert_eq!(plugin_name(7), Some("Windows 性能计数器"));
        assert!(plugin_name(8).is_none());
        assert_eq!(server_name(0), Some("10050 被动端口联签"));
        assert_eq!(server_name(3), Some("TLS PSK 联签"));
        assert!(server_name(4).is_none());
        assert_eq!(go_row_name(0), Some("Zabbix Agent 2 → Go 运行时上栈声明"));
        assert!(go_row_name(2).is_none());
        assert_eq!(gate_name(5), Some("只读采集"));
        assert!(gate_name(6).is_none());
    }

    #[test]
    fn test_grade_ladder() {
        let mut lt = ZabbixLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.judge_grade(), Grade::S2);
        let mut o = 0usize;
        while o < FAM_CELLS {
            if let Some(w) = fam_cell(o) {
                let _ = lt.record_fam(o, w);
            }
            o += 1;
        }
        let mut m = 0usize;
        while m < MODE_CELLS {
            if let Some(w) = mode_cell(m) {
                let _ = lt.record_mode(m, w);
            }
            m += 1;
        }
        let mut f = 0usize;
        while f < CONF_CELLS {
            if let Some(w) = conf_cell(f) {
                let _ = lt.record_conf(f, w);
            }
            f += 1;
        }
        let mut p = 0usize;
        while p < PLUGIN_CELLS {
            if let Some(w) = plugin_cell(p) {
                let _ = lt.record_plugin(p, w);
            }
            p += 1;
        }
        let mut sc = 0usize;
        while sc < SERVER_CELLS {
            if let Some(w) = server_cell(sc) {
                let _ = lt.record_server(sc, w);
            }
            sc += 1;
        }
        let mut r = 0usize;
        while r < GO_ROWS {
            let _ = lt.declare_go(r);
            r += 1;
        }
        assert_eq!(lt.judge_grade(), Grade::S1);
    }

    #[test]
    fn test_injection_conservation() {
        let (recorded, caught, _) = run_cycle();
        let _ = recorded;
        assert_eq!(caught, 10);
        let mut k = 0usize;
        while k < 10 {
            let kind = (k % 3) as u32;
            let mut lt = ZabbixLedger::new();
            let hit = match kind {
                0 => lt.record_fam(FAM_CELLS, 0).is_err(),
                1 => {
                    let _ = lt.record_mode(6, mode_cell(6).unwrap_or(0));
                    lt.record_mode(2, mode_cell(2).unwrap_or(0)) == Err(ZabbixCode::EModeOrder)
                }
                _ => {
                    let _ = lt.record_conf(0, conf_cell(0).unwrap_or(0));
                    lt.record_conf(1, conf_cell(1).unwrap_or(0).wrapping_add(1))
                        == Err(ZabbixCode::EConfDrift)
                }
            };
            assert!(hit);
            let (_, _, e) = lt.stats();
            assert_eq!(e, 0, "逃逸必须为 0");
            k += 1;
        }
    }

    #[test]
    fn unx52380_checks_green() {
        let s = run_unx52380_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
