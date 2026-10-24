//! UNX-F0050 · UEFI 变量只读枚举 · 诊断段 0x2030
//! 验收锚点：UNX-F0050-J1 GetNextVariableName 全链枚举走完，
//! 名称/GUID/属性三元登记无缺项，环形陷阱（链环）必检必拒；
//! 纯功能代码 ≥300 行。纪律：只读——零写入路径。


use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x2030;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NAME_LEN: u16 = SEG_BASE + 1;   // 名称长度为 0 或超上限
const E_NOTERM: u16 = SEG_BASE + 2;     // 名称窗内无 NUL 终止
const E_GUID_FMT: u16 = SEG_BASE + 3;   // GUID 混合端序不合法
const E_ATTR_UNDEF: u16 = SEG_BASE + 4; // 属性位全零（规范要求至少 NV 位语义）
const E_CYCLE: u16 = SEG_BASE + 5;      // 枚举链出现环（步数超闭集上界）
const E_DUP: u16 = SEG_BASE + 6;        // 同 (名称,GUID) 二次出现
const E_DATA_LEN: u16 = SEG_BASE + 7;   // 数据长度越窗
const E_RDONLY: u16 = SEG_BASE + 8;     // 只读账收到写请求
const E_BAL: u16 = SEG_BASE + 9;        // 枚举步数与登记数对账不平
const E_STOP: u16 = SEG_BASE + 10;      // 链尾哨兵缺失（枚举被截断）

/// 变量名 UTF-16 字符上限（UEFI 惯例 1024，本仓收紧为 256）。
pub const NAME_MAX_UNITS: usize = 256;

/// 属性位定义（UEFI 规范 §8.2）。
pub const ATTR_NV: u32 = 0x0000_0001;             // 非易失
pub const ATTR_BS: u32 = 0x0000_0002;             // BootServices 可见
pub const ATTR_RT: u32 = 0x0000_0004;             // Runtime 可见
pub const ATTR_HW_ERR: u32 = 0x0000_0008;         // 硬件错误记录
pub const ATTR_AUTH_WRITE: u32 = 0x0000_0010;     // 认证写
pub const ATTR_TIME_AUTH: u32 = 0x0000_0020;      // 时间戳认证
pub const ATTR_APPEND: u32 = 0x0000_0040;         // 只追加

/// 合法属性组合的最低要求：至少可见于 Boot 或 Runtime。
pub fn attrs_visible(a: u32) -> bool {
    a & (ATTR_BS | ATTR_RT) != 0
}

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NAME_LEN => "变量名长度为 0 或超 256 UTF-16 单元：跳过并留痕",
        E_NOTERM => "名称窗内无 NUL：截断读，绝不越出窗边界",
        E_GUID_FMT => "GUID 混合端序字段非法：数据 2/3 段须按小端复核",
        E_ATTR_UNDEF => "属性位全零：至少须 NV 或可见位之一",
        E_CYCLE => "枚举链成环：步数超闭集上界，立即停止并立案",
        E_DUP => "同名同 GUID 二次出现：后到拒绝并留痕",
        E_DATA_LEN => "数据长度越窗：截断登记，账面显性",
        E_RDONLY => "只读账收到写请求：路径错误，换读写账（F0165）",
        E_BAL => "枚举步数 ≠ 登记数 + 环拦数：对账不平",
        E_STOP => "链尾哨兵缺失：枚举被截断，总数不可信",
        _ => "未知诊断码",
    }
}

// ── 变量记录 ─────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq)]
pub struct VarRecord {
    pub name: String,
    pub guid: [u8; 16],
    pub attrs: u32,
    pub data_len: usize,
    pub data_fp: u64, // 数据 FNV 指纹（只存指纹不存数据，快照最小化）
}

impl VarRecord {
    pub fn key_fp(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(self.name.as_bytes());
        buf.extend_from_slice(&self.guid);
        fnv1a64(&buf)
    }
}

/// 解析名称：UTF-16LE 零终止，硬窗限长。
pub fn parse_name16(raw: &[u8], window_units: usize) -> Result<String, u16> {
    if window_units == 0 || window_units > NAME_MAX_UNITS {
        return Err(E_NAME_LEN);
    }
    if raw.len() < window_units * 2 {
        return Err(E_NOTERM);
    }
    let mut units: Vec<u16> = Vec::new();
    let mut terminated = false;
    for i in 0..window_units {
        let u = u16::from_le_bytes([raw[i * 2], raw[i * 2 + 1]]);
        if u == 0 {
            terminated = true;
            break;
        }
        units.push(u);
    }
    if units.is_empty() {
        return Err(E_NAME_LEN);
    }
    if !terminated {
        return Err(E_NOTERM);
    }
    let mut s = String::new();
    for u in &units {
        match char::from_u32(*u as u32) {
            Some(c) if !c.is_control() => s.push(c),
            _ => s.push('\u{FFFD}'),
        }
    }
    Ok(s)
}

/// GUID 合法性：数据 1 段任意，数据 2/3 段按小端解读后非零即可读。
/// 规范的 GUID 内存布局是混合端序；快照只验非零与长度。
pub fn validate_guid(g: &[u8; 16]) -> Result<(), u16> {
    if g.iter().all(|b| *b == 0) {
        return Err(E_GUID_FMT);
    }
    Ok(())
}

// ── 只读枚举账 ───────────────────────────────────────────────
pub struct ReadOnlyVarLedger {
    pub records: Vec<VarRecord>,
    pub steps: usize,           // GetNext 步数
    pub rejected: Vec<(String, u16)>,
    pub cycle_detected: bool,
    pub tail_ok: bool,
    pub closed: bool,
}

impl ReadOnlyVarLedger {
    pub fn new() -> ReadOnlyVarLedger {
        ReadOnlyVarLedger {
            records: Vec::new(),
            steps: 0,
            rejected: Vec::new(),
            cycle_detected: false,
            tail_ok: false,
            closed: false,
        }
    }

    /// 吸收一步枚举结果（名称/GUID/属性/数据指纹）。
    /// `steps_so_far` 供环检测：步数超闭集上界按环处理。
    pub fn absorb(
        &mut self,
        name: String,
        guid: [u8; 16],
        attrs: u32,
        data_len: usize,
        data_fp: u64,
        max_steps: usize,
    ) -> Result<bool, u16> {
        if self.closed {
            return Err(E_RDONLY);
        }
        self.steps += 1;
        if self.steps > max_steps {
            self.cycle_detected = true;
            return Err(E_CYCLE);
        }
        if name.is_empty() || name.chars().count() > NAME_MAX_UNITS {
            self.rejected.push((name, E_NAME_LEN));
            return Ok(false);
        }
        validate_guid(&guid)?;
        if attrs == 0 || !attrs_visible(attrs) {
            self.rejected.push((name, E_ATTR_UNDEF));
            return Ok(false);
        }
        let rec = VarRecord {
            name,
            guid,
            attrs,
            data_len,
            data_fp,
        };
        if self.records.iter().any(|r| r.key_fp() == rec.key_fp()) {
            self.rejected.push((rec.name.clone(), E_DUP));
            return Ok(false);
        }
        self.records.push(rec);
        Ok(true)
    }

    /// 收链：枚举正常结束（哨兵到站）后冻结账。
    pub fn close(&mut self) -> Result<(), u16> {
        if self.cycle_detected {
            return Err(E_CYCLE);
        }
        self.tail_ok = true;
        self.closed = true;
        if self.steps != self.records.len() + self.rejected.len() {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn find(&self, name: &str) -> Option<&VarRecord> {
        self.records.iter().find(|r| r.name == name)
    }

    /// 属性分类计数：NV / 易失 / 认证写 / 只追加。
    pub fn classify(&self) -> (usize, usize, usize, usize) {
        let mut nv = 0;
        let mut vol = 0;
        let mut auth = 0;
        let mut append = 0;
        for r in &self.records {
            if r.attrs & ATTR_NV != 0 {
                nv += 1;
            } else {
                vol += 1;
            }
            if r.attrs & (ATTR_AUTH_WRITE | ATTR_TIME_AUTH) != 0 {
                auth += 1;
            }
            if r.attrs & ATTR_APPEND != 0 {
                append += 1;
            }
        }
        (nv, vol, auth, append)
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for r in &self.records {
            buf.extend_from_slice(&r.key_fp().to_le_bytes());
            buf.extend_from_slice(&r.attrs.to_le_bytes());
            buf.extend_from_slice(&(r.data_len as u64).to_le_bytes());
            buf.extend_from_slice(&r.data_fp.to_le_bytes());
        }
        buf.extend_from_slice(&(self.steps as u64).to_le_bytes());
        fnv1a64(&buf)
    }
}

/// 演练用确定性变量集构造：把 (名称/GUID/属性/数据) 编为账面记录并吸收。
pub fn synth_var(name: &str, guid: [u8; 16], attrs: u32, data: &[u8]) -> (String, [u8; 16], u32, usize, u64) {
    (name.to_string(), guid, attrs, data.len(), fnv1a64(data))
}

/// 演练辅助：合成 + 吸收一步到位。
pub fn absorb_synth(
    led: &mut ReadOnlyVarLedger,
    name: &str,
    guid: [u8; 16],
    attrs: u32,
    data: &[u8],
    max_steps: usize,
) -> Result<bool, u16> {
    let (n, g, a, l, f) = synth_var(name, guid, attrs, data);
    led.absorb(n, g, a, l, f, max_steps)
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let g0: [u8; 16] = [
        0x8b, 0xe4, 0xdf, 0x61, 0x93, 0xca, 0x11, 0xd2,
        0xaa, 0x0d, 0x00, 0xe0, 0x98, 0x03, 0x2b, 0x8c, // 全局变量命名空间
    ];

    // R1：三变量全链枚举 + 收链对账。
    let mut led = ReadOnlyVarLedger::new();
    let (n1, g1, a1, d1l, d1f) = synth_var("BootOrder", g0, ATTR_NV | ATTR_BS | ATTR_RT, &[0x01, 0x00, 0x02, 0x00]);
    let (n2, g2, a2, d2l, d2f) = synth_var("SecureBoot", g0, ATTR_RT | ATTR_BS, &[0x01]);
    let (n3, g3, a3, d3l, d3f) = synth_var("PlatformLang", g0, ATTR_NV | ATTR_BS, &[0x65, 0x6e]);
    led.absorb(n1, g1, a1, d1l, d1f, 64)?;
    led.absorb(n2, g2, a2, d2l, d2f, 64)?;
    led.absorb(n3, g3, a3, d3l, d3f, 64)?;
    led.close()?;
    if led.records.len() != 3 || led.steps != 3 {
        return Err(E_BAL);
    }
    if led.find("SecureBoot").is_none() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：属性分类计数守恒。
    let (nv, vol, auth, append) = led.classify();
    if nv != 2 || vol != 1 || auth != 0 || append != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：指纹对拍——同输入两次枚举账指纹一致。
    let mut led2 = ReadOnlyVarLedger::new();
    absorb_synth(&mut led2, "BootOrder", g0, ATTR_NV | ATTR_BS | ATTR_RT, &[0x01, 0x00, 0x02, 0x00], 64)?;
    absorb_synth(&mut led2, "SecureBoot", g0, ATTR_RT | ATTR_BS, &[0x01], 64)?;
    absorb_synth(&mut led2, "PlatformLang", g0, ATTR_NV | ATTR_BS, &[0x65, 0x6e], 64)?;
    led2.close()?;
    if led.fingerprint() != led2.fingerprint() {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：属性全零 → 拒绝留痕。
    let mut l3 = ReadOnlyVarLedger::new();
    l3.absorb("Bad".to_string(), g0, 0, 1, 7, 64)?;
    if l3.rejected.len() != 1 || l3.rejected[0].1 != E_ATTR_UNDEF {
        return Err(E_ATTR_UNDEF);
    }
    hits += 1;

    // I2：同名同 GUID 二次出现 → E_DUP。
    let mut l4 = ReadOnlyVarLedger::new();
    absorb_synth(&mut l4, "X", g0, ATTR_NV | ATTR_BS, &[1], 64)?;
    absorb_synth(&mut l4, "X", g0, ATTR_NV | ATTR_BS, &[2], 64)?;
    if l4.records.len() != 1 || l4.rejected.len() != 1 {
        return Err(E_DUP);
    }
    hits += 1;

    // I3：步数超闭集 → E_CYCLE 且账面立案。
    let mut l5 = ReadOnlyVarLedger::new();
    let mut cyc: u16 = 0;
    for i in 0..6u16 {
        match l5.absorb(format!("V{}", i), g0, ATTR_NV | ATTR_BS, 1, i as u64, 4) {
            Ok(_) => {}
            Err(c) => {
                cyc = c;
                break;
            }
        }
    }
    if cyc != E_CYCLE || !l5.cycle_detected {
        return Err(E_CYCLE);
    }
    hits += 1;

    // I4：空名 → E_NAME_LEN 留痕。
    let mut l6 = ReadOnlyVarLedger::new();
    l6.absorb(String::new(), g0, ATTR_NV | ATTR_BS, 1, 9, 64)?;
    if l6.rejected.len() != 1 || l6.rejected[0].1 != E_NAME_LEN {
        return Err(E_NAME_LEN);
    }
    hits += 1;

    // I5：收链后写 → E_RDONLY。
    if led.absorb("Late".to_string(), g0, ATTR_NV | ATTR_BS, 1, 1, 64) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    // I6：全零 GUID → E_GUID_FMT（错误路径冒泡）。
    let mut l7 = ReadOnlyVarLedger::new();
    if l7.absorb("Z".to_string(), [0u8; 16], ATTR_NV | ATTR_BS, 1, 1, 64) != Err(E_GUID_FMT) {
        return Err(E_GUID_FMT);
    }
    hits += 1;

    Ok(hits)
}
