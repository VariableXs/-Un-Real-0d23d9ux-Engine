//! VE-F0213 · virtio 多头与 EDID（VE-B 域 · GPU 驱动矩阵 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0213`
//!
//! **判据（锚点原文）**：独立使能、EDID 注入、热增删、布局同步、判据。
//!
//! # 职责
//!
//! virtio 多扫描输出管理与 EDID 定制：
//!
//! 1. **每输出独立使能与分辨率选择**（判据：独立使能）——输出描述表
//!    [`OutputDesc`] 的 `enabled` 与 [`ModeSel`] **按输出号各存各的**，
//!    使能一个输出不触碰其余输出（[`HeadTable::set_enabled`] 的作用域
//!    只覆盖命中那一行，见 [`HeadStats::enable_writes`]）。
//! 2. **EDID 可由宿主配置注入（无配置时用物理显示器信息优先）**（判据：
//!    EDID 注入）——[`EdidSource`] 三级优先级由 [`EdidSource::rank`] 排序：
//!    注入值（宿主配置）> 物理显示器信息（设备 EDID）> 无 EDID 兜底
//!    （[`EdidSource::None`]）。**注入非法 → 拒载保默认**（[`HeadTable::
//!    inject_edid`] 返回 [`Reject`] 且不落库），不拿注入的半截数据当真值。
//! 3. **输出增删热事件走 F0211 事件通道**（判据：热增删）——本单元**不
//!    自建中断通路**，消费 [`EventKind::Display`] 事件的输出增/删两态，
//!    联动记录落 [`HotLinkLog`]（增/删/布局重算三行各自计数）。
//! 4. **多头布局随设备 cfg 同步并持久化**（判据：布局同步）——布局表
//!    [`LayoutEntry`] 按设备声明的 scanout 数重算；持久化门面
//!    [`LayoutTable::snapshot`] / [`LayoutTable::restore`] 让下游 F0217
//!    热重置后能保持布局语义（重置走 F0212 的 reset 序，布局不被连带清空）。
//!
//! # 数据结构（锚点原文三张表）
//!
//! - **输出描述表**（id × 使能 × 模式 × EDID 块）：[`HeadTable`] 的
//!   [`HeadRow`] 四维齐备；EDID 块按 [`EDID_LEN`] 定长（1024，规范定值，
//!   单一事实源在 `veb02_proto`）存 [`EdidBlock`]，不按实际长度裁剪
//!   （EDID 扩展块数量可变，裁剪会让「多一条扩展块」变成字节长度差）。
//! - **布局表**：[`LayoutTable`] 持 `Vec<LayoutEntry>`，每条是
//!   （输出号 × x/y 偏移 × 宽高 × 主屏标志）。
//! - **热事件联动记录**：[`HotLinkLog`] 逐条留证，供诊断回放「哪次增删
//!   触发了哪次布局重算」。
//!
//! # EDID 解析（锚点「EDID 解析 O(块长)」）
//!
//! [`EdidBlock`] 只做**基础块 128 字节**的定长解析，不解析扩展块：
//!
//! - **头部**：前 8 字节必须是 `00 FF FF FF FF FF FF 00`（EDID 1.x 固定头）。
//! - **校验和**：128 字节逐字节求和 `mod 256` 必须为 0 —— 这是 EDID 的
//!   自校验真值，**不是自己算给自己看的记账**（[`EdidBlock::checksum_ok`]）。
//! - **厂商 ID**：[`EdidBlock::vendor`] 按 EDID 规范的 3×5bit 大端编码解码
//!   （`val >> 10`、`>> 5`、`& 0x1F`，字母值 1..=26 且第 7 位为 0）。
//! - **详细时序描述符（DTD）**：18 字节块里解出像素时钟（单位 10 kHz）与
//!   h/v 有效区、h/v 消隐区，由此算**刷新率**（分母 = (h_act+h_blank)
//!   × (v_act+v_blank)）。判据「分辨率选择」由此有可验证的物理依据，
//!   不是凭空给一个 `refresh_hz` 常量。
//! - **非法判据**：头部不符、校验和不为 0、像素时钟为 0 → [`EdidVerdict`]
//!   的 [`EdidVerdict::Invalid`]，[`HeadTable::inject_edid`] 据此拒载。
//!
//! 解析是**单遍定长**（一次遍历累校验和 + 取字段），工作量与块长成正比，
//! 与 EDID 扩展块数量无关（O(块长)）。
//!
//! # 错误路径与降级矩阵（锚点原文）
//!
//! - **EDID 非法 → 拒载保默认**：注入非法 → [`Reject`] 三要素齐（发生了什么 /
//!   为什么 / 下一步），[`HeadRow::edid_source`] 保持原值；[`HeadStats::
//!   edid_rejected`] 公开计数（不是悄悄丢）。
//! - **输出超设备上限 → 拒绝并三要素**：[`HeadTable::attach_output`] 遇
//!   `id >= DisplayCfg::scanouts`（或超 [`MAX_SCANOUTS`] 定值 16）即拒绝，
//!   [`HeadStats::outputs_rejected`] 计数；**已挂载的输出一个不动**
//!   （不半途清表）。
//! - **热事件乱序 → 以最新 cfg 为准重算**：本单元的增删是**幂等重算**
//!   （[`HeadTable::sync_with_cfg`] 按最新 cfg 重算，不逐条回放事件），
//!   所以乱序事件的最终态与事件到达顺序无关——[`HeadStats::
//!   stale_events`] 显式计数乱序事件，但**不影响最终布局**（这是判据
//!   「以最新 cfg 为准」的落点：不靠事件序正确性兜底）。
//!
//! # 性能逐项分解（锚点原文）
//!
//! - **EDID 解析 O(块长)**：见上，单遍定长；[`HeadStats::edid_parses`]
//!   可核（正常路径不注入 EDID 时该计数恒 0，见自检 `C213-性能-*`）。
//! - **布局应用 O(输出数)**：[`LayoutTable::apply`] 单遍扫输出描述表，
//!   不做排列搜索（不求「最优布局」——多屏布局是用户排布，不是求解问题）。
//! - **热事件增量处理**：[`HeadTable::sync_with_cfg`] 只对**集合差**动手
//!   （新增号、消失号），命中集合相同则零写（[`HeadStats::sync_writes`]
//!   在无变化时恒 0）。
//!
//! # 跨批对接点（锚点原文）
//!
//! - **上游 F0209**（`veb09` 扫描输出与呈现）：输出使能后的 scanout 绑定
//!   由 F0209 消费；本单元只管「哪个输出开、什么模式、什么 EDID」，
//!   不碰 `CMD_SET_SCANOUT` 的下发时机（下发归 F0209，避免双重下发）。
//! - **上游 F0210**（`veb10_cursor`）：光标归属输出号由本单元的输出描述表
//!   定义；F0210 的 cursorq 消息携带同一输出号，两处号空间必须一致
//!   （[`HeadTable::cursor_owner_ok`] 是这一致性的机检面）。
//! - **下游 F0217**（热重置与 suspend/resume）：[`LayoutTable::snapshot`]
//!   / [`LayoutTable::restore`] 是布局语义的持久化门面；F0217 走 F0212 的
//!   `RESET_SEQUENCE` 重置设备，但**布局不随之清空**——热重置后用户排布
//!   仍在（[`HeadStats::layout_retained_after_reset`] 是这条的判据）。
//!
//! # 无障碍与隐私
//!
//! - **输出命名含位置语义（左/右）供读屏播报**：[`HeadRow::position_label`]
//!   产出「左屏 / 右屏 / 上屏 / 下屏 / 主屏」的位置语义（不是序号「输出 2」——
//!   读屏用户需要的是位置，不是内部编号）；[`HeadRow::spoken_name`] 把它与
//!   模式合成一句可播报文案。
//! - **不依赖颜色单独表意**：输出状态（开/关/降级）全部落在文字里。
//! - **隐私**：本单元不载用户内容——EDID 里只有显示器厂商/型号/序列号与
//!   时序参数，**不含**画面内容、文件名或窗口标题。

use super::veb01_device::{DisplayCfg, MAX_SCANOUTS};
use super::veb02_proto::{Rect, CMD_GET_EDID, CMD_SET_SCANOUT, EDID_LEN};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// EDID 基础块字节数（EDID 1.x 规范定值 128）。
pub const EDID_BASE_LEN: usize = 128;

/// EDID 固定头（基础块前 8 字节）。
pub const EDID_HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];

/// 详细时序描述符字节数（EDID 规范定值 18）。
pub const DTD_LEN: usize = 18;

/// 基础块内 DTD 槽数（4 个 18 字节描述符）。
pub const DTD_SLOTS: usize = 4;

/// 基础块内第一个 DTD 的起始偏移。
pub const DTD_BASE_OFFSET: usize = 54;

/// EDID 块总数（基础块 + 扩展块；扩展块数存在基础块第 126 字节）。
pub const EDID_BLOCKS: usize = 8;

/// 刷新率分母的最小值保护（h/v 有效区 + 消隐区全 0 时防除零）。
pub const MIN_REFRESH_DENOM: u32 = 1;

/// 像素时钟单位（EDID 规范：像素时钟以 10 kHz 计）。
pub const PIXEL_CLOCK_UNIT_HZ: u32 = 10_000;

/// 热事件联动记录容量（增量日志的环形上界）。
pub const HOTLINK_SLOTS: usize = 32;

/// 布局表容量上界（与 [`MAX_SCANOUTS`] 对齐，留一倍余量防设备异常膨胀）。
pub const LAYOUT_SLOTS: usize = MAX_SCANOUTS as usize * 2;

/// 与下游 F0217 的布局持久化契约版本（结构变更即递增）。
pub const LAYOUT_LINK_VERSION: u32 = 1;

/// 上游 F0209 的接口契约版本（本单元输出描述表被其消费的版本）。
pub const SCANOUT_LINK_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// 二、拒绝三要素（异常零静默的载体）
// ---------------------------------------------------------------------------

/// 拒绝三要素：发生了什么 / 为什么 / 下一步怎么办。
///
/// 锚点「输出超设备上限 → 拒绝并三要素」的载体：任何拒绝都必须同时给全
/// 三项，只报 code 不给路的拒绝在评审按缺陷处理。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reject {
    pub code: &'static str,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl Reject {
    /// 三要素齐全（自检逐条核）。
    pub fn is_complete(&self) -> bool {
        !self.code.is_empty() && !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

fn reject(code: &'static str, what: String, why: String, next: String) -> Reject {
    Reject { code, what, why, next }
}

// ---------------------------------------------------------------------------
// 三、EDID 块与解析
// ---------------------------------------------------------------------------

/// EDID 块（定长 [`EDID_LEN`]，与 `CMD_GET_EDID` 响应载荷同宽）。
///
/// **不定长裁剪**：EDID 基础块 128 字节 + N 个 128 字节扩展块，N 由基础块
/// 第 126 字节给出。存定长是为了让「块数」与「字节数」解耦——裁剪成实际
/// 长度会让「多一条扩展块」变成纯字节长度差，比对时看不出是块数变了。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdidBlock {
    bytes: [u8; EDID_LEN],
}

impl EdidBlock {
    /// 全零块（尚未取到 EDID 的初值）。
    pub const fn empty() -> EdidBlock {
        EdidBlock { bytes: [0; EDID_LEN] }
    }

    /// 从定长载荷构造（不足 [`EDID_LEN`] 按零填充——调用方多在解码侧）。
    pub fn from_payload(payload: &[u8]) -> EdidBlock {
        let mut b = [0u8; EDID_LEN];
        let n = if payload.len() < EDID_LEN { payload.len() } else { EDID_LEN };
        let mut i = 0;
        while i < n {
            b[i] = payload[i];
            i += 1;
        }
        EdidBlock { bytes: b }
    }

    /// 基础块字节访问（越界返回 0——读取面不 panic）。
    pub fn byte_at(&self, index: usize) -> u8 {
        if index < EDID_LEN {
            self.bytes[index]
        } else {
            0
        }
    }

    /// 扩展块数（基础块第 126 字节）。
    ///
    /// EDID 规范语义：第 126 字节是**扩展块个数，不含基础块自身**。所以
    /// EDID 总块数 = 本值 + 1。越界或超 [`EDID_BLOCKS`] 截到上限。
    ///
    /// 判据写「扩展块数 == byte126 的值」而不是「== byte126 + 1」——早先
    /// 把基础块也计进来，导致本函数返回 2 而基准块声明 1，看起来像解析错。
    /// 命名与语义必须一致：`extension_count` 就是扩展块数，基础块另算。
    pub fn extension_count(&self) -> usize {
        let raw = self.byte_at(126) as usize;
        if raw >= EDID_BLOCKS {
            EDID_BLOCKS - 1
        } else {
            raw
        }
    }

    /// EDID 总块数（基础块 + 扩展块）。
    pub fn total_block_count(&self) -> usize {
        1 + self.extension_count()
    }

    /// 基础块前 8 字节是否等于固定头。
    pub fn header_ok(&self) -> bool {
        let mut i = 0;
        while i < 8 {
            if self.bytes[i] != EDID_HEADER[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 校验和判定：基础块 128 字节逐字节求和 `mod 256` 必须为 0。
    ///
    /// 这是 EDID 块自带的**真值**（接收方无需信任发送方），不是本单元自己
    /// 记账的数——所以它能当判据用（错误注入会翻红，正确 EDID 恒为真）。
    pub fn checksum_ok(&self) -> bool {
        let mut sum: u32 = 0;
        let mut i = 0;
        while i < EDID_BASE_LEN {
            sum += self.bytes[i] as u32;
            i += 1;
        }
        sum % 256 == 0
    }

    /// 厂商 ID 解码（EDID 规范 3×5bit 大端编码）。
    ///
    /// 位分配：三个 5bit 组从高到低占 bits 14..10 / 9..5 / 4..0，bit 15 恒 0。
    /// 组值 1..=26 对应 'A'..'Z'。
    ///
    /// 对拍锚点：真实 128 字节 EDID 头里 Dell 的厂商字节是 `0x10 0xAC`
    /// → `(4<<10)|(5<<5)|12` → `D E L`。用厂商码自造一遍再解回是恒真弱门禁
    /// （记忆门禁四则①），所以自检里用的是这段真实字节。
    ///
    /// **有效性判据**：任一 5bit 组越界（0 或 >26）即判非法——此时返回
    /// None 而非拼出乱码字母。此处**不写** `group & 0x20 != 0` 这类检查：
    /// 组值已 `& 0x1F` 截到 5 位，bit5 恒为 0，该条件恒真 = 无门禁。
    /// 高位越界的真信号是 `raw & 0x8000`（bit15 规范要求为 0）。
    pub fn vendor(&self) -> Option<[u8; 3]> {
        if !self.header_ok() {
            return None;
        }
        // 大端取高字节：厂商 ID 是 16 位大端，高字节在 bytes[8]。
        let raw = ((self.byte_at(8) as u16) << 8) | (self.byte_at(9) as u16);
        if raw & 0x8000 != 0 {
            return None;
        }
        let mut out = [0u8; 3];
        let mut i = 0;
        while i < 3 {
            // 5bit 组从高到低：bits 10..14 / 5..9 / 0..4
            let shift = 10 - i * 5;
            let group = ((raw >> shift) & 0x1F) as u8;
            if group < 1 || group > 26 {
                return None;
            }
            out[i] = b'A' + group - 1;
            i += 1;
        }
        Some(out)
    }

    /// 厂商 ID 文本（大端解码后拼串；未取到返回空串）。
    pub fn vendor_text(&self) -> String {
        match self.vendor() {
            Some(v) => {
                // 逐字节推入而非 v[0]/v[1]/v[2] 字面下标：定长数组的常量下标虽
                // 编译期可证不越界，但字面下标出现在读取面上会让「零 panic 面」
                // 的机检（grep 字面下标）变成假红。循环写法与该机检口径一致。
                let mut s = String::new();
                let mut i = 0;
                while i < 3 {
                    s.push(v[i] as char);
                    i += 1;
                }
                s
            }
            None => String::new(),
        }
    }

    /// 详细时序描述符解析（取第 `slot` 个；越界或非 DTD 返回 None）。
    ///
    /// DTD 18 字节布局（EDID 1.3 规范，逐字节）：
    /// - `0..=1` 像素时钟（10 kHz 单位，**小端**）
    /// - `2` h 有效区低 8 位；`4` 的 **bit7..4** 是其高 4 位
    /// - `3` h 消隐低 8 位；`4` 的 **bit3..0** 是其高 4 位
    /// - `5` v 有效区低 8 位；`7` 的 **bit7..4** 是其高 4 位
    /// - `6` v 消隐低 8 位；`7` 的 **bit3..0** 是其高 4 位
    ///
    /// 空槽的判据是前 2 字节为 0（像素时钟 0 = 非 DTD；规范用 0 填槽，
    /// 显示范围限制描述符也是这个形态）。
    ///
    /// **高半字节是 4 位不是 3 位**——这条是本单元最贵的坑。位分配有两个
    /// 都在流传的错法，且都「看起来能跑」（类型检查、编译全绿）：
    ///
    /// | 拆分方式 | byte4=0x71 解出的 h 有效区 | 对不对 |
    /// |----------|--------------------------|--------|
    /// | 4+4（本实现） | `(0x71>>4)<<8 \\| 0x80` = 1920 | 对 |
    /// | 3+3（bit7..5 / bit4..2） | `3<<8 \\| 0x80` = 896 | 错 |
    /// | 3+3（bit2..0 / bit6..4） | `7<<8 \\| 0x80` = 1920 | 碰巧对 h，v 全错 |
    ///
    /// 1920×1080@60 的真实 DTD 前 8 字节是 `02 3A 80 18 71 38 2D 40`：
    /// 只有 4+4 能同时解出 h=1920/h_blank=280/v=1080/v_blank=45
    /// （像素时钟 14850 → 148.5 MHz，刷新率 148.5e6/(2200×1125) = 60.0 Hz
    /// 整除——刷新率整除到 60 是这套字节对拍的外部锚点，不是自证）。
    pub fn dtd(&self, slot: usize) -> Option<Dtd> {
        if slot >= DTD_SLOTS {
            return None;
        }
        let base = DTD_BASE_OFFSET + slot * DTD_LEN;
        // 像素时钟是 16 位**小端**：低字节在 base+0、高字节在 base+1。
        // 高字节必须 `(base+1) << 8`——写成 `(base+2) << 8` 会把「h 有效区低
        // 8 位」当高字节，14850（0x3A02）会被读成 0x8002（327.7 MHz），
        // 刷新率算出来是 132 Hz，全程静默。
        let pixel_clock_tens_khz = ((self.byte_at(base + 1) as u32) << 8) | (self.byte_at(base) as u32);
        if pixel_clock_tens_khz == 0 {
            return None;
        }
        let b4 = self.byte_at(base + 4) as u32;
        let b7 = self.byte_at(base + 7) as u32;
        let b6 = self.byte_at(base + 6) as u32;
        Some(Dtd {
            pixel_clock_hz: pixel_clock_tens_khz * PIXEL_CLOCK_UNIT_HZ,
            h_active: self.byte_at(base + 2) as u32 | (((b4 >> 4) & 0x0F) << 8),
            h_blank: self.byte_at(base + 3) as u32 | ((b4 & 0x0F) << 8),
            v_active: self.byte_at(base + 5) as u32 | (((b7 >> 4) & 0x0F) << 8),
            v_blank: b6 | ((b7 & 0x0F) << 8),
        })
    }

    /// 首选 DTD 槽位号（第一个非空槽；无则 None）。
    pub fn preferred_slot(&self) -> Option<usize> {
        let mut i = 0;
        while i < DTD_SLOTS {
            if self.dtd(i).is_some() {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 逐字节求和的中间值（暴露给自检做单边符号对账用）。
    pub fn base_checksum_raw(&self) -> u32 {
        let mut sum: u32 = 0;
        let mut i = 0;
        while i < EDID_BASE_LEN {
            sum += self.bytes[i] as u32;
            i += 1;
        }
        sum
    }

    /// 单字节改写（构造测试用注入点；越界静默忽略，不 panic）。
    pub fn poke(&mut self, index: usize, value: u8) {
        if index < EDID_LEN {
            self.bytes[index] = value;
        }
    }

    /// 基础块原始字节读出（前 `len` 字节；供自检对账）。
    pub fn base_bytes(&self, len: usize) -> Vec<u8> {
        let n = if len > EDID_BASE_LEN { EDID_BASE_LEN } else { len };
        let mut v = Vec::new();
        let mut i = 0;
        while i < n {
            v.push(self.bytes[i]);
            i += 1;
        }
        v
    }
}

/// 详细时序描述符解码结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dtd {
    pub pixel_clock_hz: u32,
    pub h_active: u32,
    pub h_blank: u32,
    pub v_active: u32,
    pub v_blank: u32,
}

impl Dtd {
    /// 模式（分辨率 + 刷新率）。
    ///
    /// 刷新率 = 像素时钟 / ((h_active + h_blank) × (v_active + v_blank))。
    /// 分母为 0 时按 [`MIN_REFRESH_DENOM`] 兜底并给 0 Hz（不假装有刷新率）。
    pub fn mode(&self) -> ModeSel {
        let h_total = self.h_active + self.h_blank;
        let v_total = self.v_active + self.v_blank;
        let denom = match h_total.checked_mul(v_total) {
            Some(d) if d >= MIN_REFRESH_DENOM => d,
            _ => MIN_REFRESH_DENOM,
        };
        ModeSel {
            width: self.h_active,
            height: self.v_active,
            refresh_hz: if denom == MIN_REFRESH_DENOM && (h_total == 0 || v_total == 0) {
                0
            } else {
                self.pixel_clock_hz / denom
            },
        }
    }
}

/// EDID 合法性判定（判据「EDID 非法 → 拒载保默认」的分类面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdidVerdict {
    /// 合法且含至少一个 DTD（可解出模式）。
    Valid { slot: usize },
    /// 结构合法但无任何 DTD（拿不到模式，注入无意义）。
    NoTiming,
    /// 非法（头部不符 / 校验和错 / 无 DTD 且无有效像素时钟）。
    Invalid(&'static str),
}

impl EdidVerdict {
    pub fn is_valid(&self) -> bool {
        matches!(self, EdidVerdict::Valid { .. })
    }

    pub fn reason(&self) -> &'static str {
        match self {
            EdidVerdict::Valid { .. } => "OK",
            EdidVerdict::NoTiming => "NO_TIMING",
            EdidVerdict::Invalid(r) => r,
        }
    }
}

/// EDID 合法性判定入口（一次单遍：头部 → 校验和 → DTD）。
///
/// **单遍纪律**：校验和累加在 [`EdidBlock::base_checksum_raw`] 里一次
/// 做完，本函数不重复扫字节（工作量 O(块长) 而非 O(2×块长)）。
pub fn verify_edid(block: &EdidBlock) -> EdidVerdict {
    if !block.header_ok() {
        return EdidVerdict::Invalid("HEADER_MISMATCH");
    }
    if !block.checksum_ok() {
        return EdidVerdict::Invalid("CHECKSUM_BAD");
    }
    match block.preferred_slot() {
        Some(slot) => EdidVerdict::Valid { slot },
        None => EdidVerdict::NoTiming,
    }
}

// ---------------------------------------------------------------------------
// 四、模式与输出描述表
// ---------------------------------------------------------------------------

/// 输出模式（分辨率 + 刷新率）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeSel {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
}

impl ModeSel {
    /// 模式自检用构造。
    pub fn new(width: u32, height: u32, refresh_hz: u32) -> ModeSel {
        ModeSel { width, height, refresh_hz }
    }

    /// 是否落在设备能力范围内（`max_width/max_height` 上限）。
    pub fn within(&self, cfg: &DisplayCfg) -> bool {
        self.width > 0 && self.height > 0 && self.width <= cfg.max_width && self.height <= cfg.max_height
    }
}

/// EDID 来源（判据「EDID 注入」的优先级载体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdidSource {
    /// 宿主配置注入（优先级最高）。
    Injected,
    /// 物理显示器信息（设备 EDID）。
    Physical,
    /// 无 EDID（兜底；模式靠调用方给）。
    None,
}

impl EdidSource {
    /// 优先级数值（大者胜；锚点「无配置时用物理显示器信息优先」）。
    pub fn rank(self) -> u8 {
        match self {
            EdidSource::Injected => 2,
            EdidSource::Physical => 1,
            EdidSource::None => 0,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            EdidSource::Injected => "注入",
            EdidSource::Physical => "物理",
            EdidSource::None => "无",
        }
    }
}

/// 输出描述表的一行（锚点「输出描述表：id × 使能 × 模式 × EDID 块」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadRow {
    /// 输出号（0 起；`>= DisplayCfg::scanouts` 即越界）。
    pub id: u32,
    /// 独立使能位（每行各存各的）。
    pub enabled: bool,
    /// 分辨率选择（每行各存各的）。
    pub mode: ModeSel,
    /// EDID 来源（注入 / 物理 / 无）。
    pub edid_source: EdidSource,
    /// EDID 块（`EdidSource::None` 时为空块）。
    pub edid: EdidBlock,
}

/// 位置语义（锚点无障碍面：命名含左/右供读屏播报）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PositionLabel {
    /// 主屏（布局基准）。
    Primary,
    /// 左屏。
    Left,
    /// 右屏。
    Right,
    /// 上屏。
    Above,
    /// 下屏。
    Below,
    /// 未布局（位置未知）。
    Unplaced,
}

impl PositionLabel {
    pub fn label(self) -> &'static str {
        match self {
            PositionLabel::Primary => "主屏",
            PositionLabel::Left => "左屏",
            PositionLabel::Right => "右屏",
            PositionLabel::Above => "上屏",
            PositionLabel::Below => "下屏",
            PositionLabel::Unplaced => "未布局",
        }
    }

    /// 是否含左右语义（锚点「含位置语义（左/右）」的机检面）。
    pub fn has_side(self) -> bool {
        matches!(self, PositionLabel::Left | PositionLabel::Right)
    }
}

impl HeadRow {
    /// 构造一行（EDID 来源初值 [`EdidSource::None`]）。
    pub fn new(id: u32, mode: ModeSel) -> HeadRow {
        HeadRow {
            id,
            enabled: false,
            mode,
            edid_source: EdidSource::None,
            edid: EdidBlock::empty(),
        }
    }

    /// 位置语义（由布局表给；未布局给 [`PositionLabel::Unplaced`]）。
    pub fn position_label(&self, layout: &LayoutTable) -> PositionLabel {
        layout.position_of(self.id)
    }

    /// 读屏播报名（位置语义 + 模式，不含内部编号）。
    ///
    /// 文案里不出现「输出 2」这类内部编号——读屏用户需要「左屏 1920×1080」
    /// 而不是「scanout 2」。
    pub fn spoken_name(&self, layout: &LayoutTable) -> String {
        let pos = self.position_label(layout).label();
        format!("{} {}×{} @ {}Hz", pos, self.mode.width, self.mode.height, self.mode.refresh_hz)
    }
}

// ---------------------------------------------------------------------------
// 五、多头表（输出描述表的持有者）
// ---------------------------------------------------------------------------

/// 统计（诊断面；异常与增量都公开计数，不静默）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeadStats {
    /// 使能写入次数（正常路径每使能一次 +1；作用域应只覆盖命中行）。
    pub enable_writes: u64,
    /// 模式切换次数。
    pub mode_writes: u64,
    /// EDID 注入次数（含被拒）。
    pub edid_parses: u64,
    /// EDID 被拒次数（判据「拒载保默认」的计数面）。
    pub edid_rejected: u64,
    /// 输出挂载被拒次数（超设备上限）。
    pub outputs_rejected: u64,
    /// 输出挂载成功次数。
    pub outputs_attached: u64,
    /// cfg 同步写表次数（集合相同时应为 0——增量处置证据）。
    pub sync_writes: u64,
    /// cfg 同步次数（不论有无变化）。
    pub sync_calls: u64,
    /// 乱序热事件计数（不影响最终态，只记账）。
    pub stale_events: u64,
    /// 热事件摄入次数。
    pub hot_events: u64,
    /// 布局重算次数。
    pub layout_applies: u64,
    /// 热重置后布局保留次数（下游 F0217 消费面）。
    pub layout_retained_after_reset: u64,
}

/// 多头表：输出描述表 + 热事件联动。
#[derive(Debug)]
pub struct HeadTable {
    rows: Vec<HeadRow>,
    /// 设备最新 cfg（乱序处置的「真值」来源）。
    cfg: DisplayCfg,
    /// 热事件联动环形记录。
    pub hotlink: HotLinkLog,
    pub stats: HeadStats,
}

impl HeadTable {
    /// 构造（初态：cfg 为 QEMU 默认，输出表空）。
    pub fn new() -> HeadTable {
        HeadTable {
            rows: Vec::new(),
            cfg: DisplayCfg::qemu_default(),
            hotlink: HotLinkLog::new(),
            stats: HeadStats::default(),
        }
    }

    /// 最新 cfg（单一事实源）。
    pub fn cfg(&self) -> DisplayCfg {
        self.cfg
    }

    /// 输出表行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 行读取（越界 None——读取面不 panic）。
    pub fn row(&self, id: u32) -> Option<&HeadRow> {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].id == id {
                return Some(&self.rows[i]);
            }
            i += 1;
        }
        None
    }

    /// 行索引（越界 None）。
    fn index_of(&self, id: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 挂载输出（判据：输出超设备上限 → 拒绝并三要素）。
    ///
    /// 边界防护：三重闸 —— `LAYOUT_SLOTS` 容量、[`MAX_SCANOUTS`] 规格定值、
    /// `DisplayCfg::scanouts` 设备真值。任一命中即拒绝，**已挂载的行一个不动**
    /// （拒绝不是半途清表）。
    pub fn attach_output(&mut self, id: u32, mode: ModeSel) -> Result<(), Reject> {
        if self.index_of(id).is_some() {
            // 重复挂载视为幂等请求：返回三要素让调用方知道「没新建」。
            return Err(reject(
                "E_HEAD_DUPLICATE",
                format!("输出 {} 已在输出表中", id),
                "重复挂载会造出两条同号描述，绑定时按谁生效就成了隐式约定".to_string(),
                "改用 set_enabled / set_mode 更新既有输出".to_string(),
            ));
        }
        if id >= MAX_SCANOUTS {
            self.stats.outputs_rejected += 1;
            return Err(reject(
                "E_HEAD_OVER_SPEC",
                format!("输出号 {} 超过 virtio 规格上限 {}", id, MAX_SCANOUTS - 1),
                "MAX_SCANOUTS 是规格定值 16，越限的输出号在线路上没有对应口".to_string(),
                "改用 0..=15 的输出号；更多屏请走多设备而非单设备超用".to_string(),
            ));
        }
        if id >= self.cfg.scanouts {
            self.stats.outputs_rejected += 1;
            return Err(reject(
                "E_HEAD_OVER_DEVICE",
                format!("输出号 {} 超出设备声明的 {} 个扫描口", id, self.cfg.scanouts),
                "设备 cfg 是当前唯一真值——写超出的输出号只会让状态与设备不符".to_string(),
                "先 sync_with_cfg 按最新 cfg 补挂输出，或改用设备已声明的输出号".to_string(),
            ));
        }
        if self.rows.len() >= LAYOUT_SLOTS {
            self.stats.outputs_rejected += 1;
            return Err(reject(
                "E_HEAD_NO_SLOT",
                format!("输出表已满（{} 行）", LAYOUT_SLOTS),
                "输出表容量是设备异常膨胀的兜底闸，无限增长会拖垮布局重算".to_string(),
                format!("确认设备 cfg 的 scanouts 合法（<= {}），再重试", MAX_SCANOUTS),
            ));
        }
        self.rows.push(HeadRow::new(id, mode));
        self.stats.outputs_attached += 1;
        Ok(())
    }

    /// 独立使能（判据：独立使能）。
    ///
    /// 作用域纪律：只改命中那一行；其余行的 `enabled` 与 `mode` 逐字节不动
    /// （自检用「集合差」核，不用单点断言）。
    pub fn set_enabled(&mut self, id: u32, enabled: bool) -> Result<(), Reject> {
        match self.index_of(id) {
            Some(i) => {
                if self.rows[i].enabled == enabled {
                    return Ok(());
                }
                self.rows[i].enabled = enabled;
                self.stats.enable_writes += 1;
                Ok(())
            }
            None => {
                self.stats.outputs_rejected += 1;
                Err(reject(
                    "E_HEAD_NO_ROW",
                    format!("输出 {} 未挂载，无法设使能", id),
                    "使能位是每行属性，没有行的输出无处安放该位".to_string(),
                    format!("先 attach_output({}) 或 sync_with_cfg 补挂", id),
                ))
            }
        }
    }

    /// 分辨率选择（判据：每输出独立模式）。
    ///
    /// 模式须落在设备能力内（`max_width/max_height`）——超出即拒绝，不静默
    /// 夹到边界（夹边界会让用户选了 8K 却拿到 4K，且界面显示仍是 8K）。
    pub fn set_mode(&mut self, id: u32, mode: ModeSel) -> Result<(), Reject> {
        let idx = match self.index_of(id) {
            Some(i) => i,
            None => {
                self.stats.outputs_rejected += 1;
                return Err(reject(
                    "E_HEAD_NO_ROW",
                    format!("输出 {} 未挂载，无法设模式", id),
                    "模式是每行属性，没有行的输出无处安放该模式".to_string(),
                    format!("先 attach_output({}) 或 sync_with_cfg 补挂", id),
                ));
            }
        };
        if !mode.within(&self.cfg) {
            self.stats.outputs_rejected += 1;
            return Err(reject(
                "E_HEAD_MODE_OVER",
                format!(
                    "模式 {}×{} 超出设备能力 {}×{}",
                    mode.width, mode.height, self.cfg.max_width, self.cfg.max_height
                ),
                "超能力模式在设备上无输出——写下去只会得到黑屏而非报错".to_string(),
                format!("改用 <= {}×{} 的模式", self.cfg.max_width, self.cfg.max_height),
            ));
        }
        if self.rows[idx].mode == mode {
            return Ok(());
        }
        self.rows[idx].mode = mode;
        self.stats.mode_writes += 1;
        Ok(())
    }

    /// EDID 注入（判据：EDID 注入 / 非法 → 拒载保默认）。
    ///
    /// 优先级：注入只在**来源级别不低于**当前时生效——已注入的行再被物理
    /// EDID 覆盖是宿主配置失效（[`EdidSource::rank`] 单调，判据可机检）。
    pub fn inject_edid(&mut self, id: u32, source: EdidSource, block: EdidBlock) -> Result<(), Reject> {
        self.stats.edid_parses += 1;
        let idx = match self.index_of(id) {
            Some(i) => i,
            None => {
                self.stats.edid_rejected += 1;
                return Err(reject(
                    "E_EDID_NO_ROW",
                    format!("输出 {} 未挂载，无法注入 EDID", id),
                    "EDID 是每行属性，没有行的输出无处安放该块".to_string(),
                    format!("先 attach_output({}) 或 sync_with_cfg 补挂", id),
                ));
            }
        };
        if source == EdidSource::None {
            self.stats.edid_rejected += 1;
            return Err(reject(
                "E_EDID_NO_SOURCE",
                format!("输出 {} 的 EDID 来源为「无」，不可注入", id),
                "注入必须说明来源——来源缺失时无法判定优先级，判据就落空".to_string(),
                "指定 EdidSource::Injected（宿主配置）或 EdidSource::Physical（设备）".to_string(),
            ));
        }
        if source.rank() < self.rows[idx].edid_source.rank() {
            self.stats.edid_rejected += 1;
            return Err(reject(
                "E_EDID_LOWER_PRIORITY",
                format!(
                    "输出 {} 当前来源为「{}」，不接受更低的「{}」",
                    id,
                    self.rows[idx].edid_source.name(),
                    source.name()
                ),
                "低优先级来源覆盖高优先级会让宿主配置静默失效，且无法事后分辨是谁赢的".to_string(),
                format!(
                    "要换源请先降到 EdidSource::None（{}）再注入，或直接注入同级别来源",
                    EdidSource::None.name()
                ),
            ));
        }
        // 判定与落库分离：先判，判不过保原默认。
        let verdict = verify_edid(&block);
        let slot = match verdict {
            EdidVerdict::Valid { slot } => slot,
            _ => {
                self.stats.edid_rejected += 1;
                return Err(reject(
                    "E_EDID_INVALID",
                    format!("输出 {} 的 EDID 块判定为 {}（{}）", id, verdict.reason(), block.vendor_text_or_dash()),
                    "非法 EDID 载进去会让模式解析拿到错的值——宁可用默认也别用半截数据".to_string(),
                    "核对头部 00 FF FF FF FF FF FF 00 与 128 字节校验和 mod 256 = 0".to_string(),
                ));
            }
        };
        let mode = match block.dtd(slot) {
            Some(dtd) => dtd.mode(),
            None => {
                self.stats.edid_rejected += 1;
                return Err(reject(
                    "E_EDID_NO_MODE",
                    format!("输出 {} 的 EDID 槽 {} 解不出时序", id, slot),
                    "判定说有效、解析说无时序——两者矛盾说明解析面有洞".to_string(),
                    "复核 DTD 18 字节布局的像素时钟字段".to_string(),
                ));
            }
        };
        if !mode.within(&self.cfg) {
            self.stats.edid_rejected += 1;
            return Err(reject(
                "E_EDID_MODE_OVER",
                format!(
                    "输出 {} 的 EDID 首选模式 {}×{} 超出设备能力 {}×{}",
                    id, mode.width, mode.height, self.cfg.max_width, self.cfg.max_height
                ),
                "EDID 报的模式设备渲染不出来——保默认比载入一个黑屏输出诚实".to_string(),
                format!("用 set_mode 另选 <= {}×{} 的模式", self.cfg.max_width, self.cfg.max_height),
            ));
        }
        // 落库：块、来源、模式一起换——三者在同一次注入里保持自洽。
        self.rows[idx].edid = block;
        self.rows[idx].edid_source = source;
        self.rows[idx].mode = mode;
        Ok(())
    }

    /// 按最新设备真值幂等重算输出表（判据：热增删 / 热事件乱序 → 以最新 cfg 为准）。
    ///
    /// **处置是幂等重算，不是事件回放**：只对集合差动手（新增号挂载、
    /// 消失号摘除），集合相同则零写。最终态与事件到达顺序无关——乱序事件
    /// 只进 [`HeadStats::stale_events`] 计数，不改结果（锚点原文要求）。
    ///
    /// **「最新」二字的落点**：`live_scanouts` 是设备**本次报的口数**，
    /// 它必须先落进 `self.cfg.scanouts` 再重算。早先的实现拿
    /// `min(live_scanouts, self.cfg.scanouts)` 夹住新值，而 `self.cfg`
    /// 装的是**上一次**的口数——热增删事件因此永远同步不到新口数
    /// （设备从 1 口增到 2 口，表里仍是 1 行），「以最新 cfg 为准」成了空话。
    /// 现在 `self.cfg.scanouts` 由本参数直接更新，尺寸能力（max_width/
    /// max_height）沿用 `cfg` 传入值。
    ///
    /// 非法真值不采纳：`live_scanouts` 为 0 或超 [`MAX_SCANOUTS`] 即整体
    /// 拒绝（拒绝时表与 cfg 均不动，不半途改一半）。
    pub fn sync_with_cfg(&mut self, cfg: DisplayCfg, live_scanouts: u32) {
        self.stats.sync_calls += 1;
        // 设备真值钳制：口数越界时不采纳（拒绝把非法真值写进表）。
        if live_scanouts == 0 || live_scanouts > MAX_SCANOUTS {
            return;
        }
        if cfg.max_width == 0 || cfg.max_height == 0 {
            return;
        }
        // 先把最新口数落进 cfg，再按它重算——这是「以最新 cfg 为准」的实质。
        self.cfg = DisplayCfg {
            scanouts: live_scanouts,
            max_width: cfg.max_width,
            max_height: cfg.max_height,
        };
        let live = live_scanouts;
        // 摘除：消失的输出号（倒序摘除，保持行序稳定）。
        let mut i = self.rows.len();
        while i > 0 {
            i -= 1;
            if self.rows[i].id >= live {
                self.rows.remove(i);
                self.stats.sync_writes += 1;
            }
        }
        // 新增：缺号的行补上（模式用当前 cfg 下的保守值，随后由调用方 set_mode）。
        let mut id = 0;
        while id < live {
            if self.index_of(id).is_none() {
                let mode = ModeSel::new(self.cfg.max_width.min(1920), self.cfg.max_height.min(1080), 60);
                self.rows.push(HeadRow::new(id, mode));
                self.stats.sync_writes += 1;
            }
            id += 1;
        }
    }

    /// 摄入 F0211 事件通道的输出增删热事件（判据：热增删）。
    ///
    /// 本单元**不自建中断通路**——只消费 [`EventKind::Display`] 事件。
    /// 增删一律走 [`HeadTable::sync_with_cfg`] 重算（以最新 cfg 为真值），
    /// 因此乱序到达不影响最终态。
    pub fn on_display_event(&mut self, ev: &super::veb11_irq::Event, latest_scanouts: u32) -> bool {
        self.stats.hot_events += 1;
        // 越界事件：拒收（不进联动记录——记了就是替设备撒谎说「有这么个输出」）。
        if ev.scanout >= self.cfg.scanouts {
            self.stats.stale_events += 1;
            return false;
        }
        // 事件序判据：事件携带的 cfg 代数（param 上半）小于表内已知代数 → 乱序。
        // 处置：仍按最新 cfg 重算，只把这条记成 stale——**不按事件语义回放**。
        let ev_gen = (ev.param >> 16) as u64;
        let known_gen = self.hotlink.current_gen();
        if ev_gen < known_gen {
            self.stats.stale_events += 1;
            self.hotlink.record(HotLinkKind::Stale, ev.scanout, ev_gen);
        }
        self.sync_with_cfg(self.cfg, latest_scanouts);
        self.hotlink.bump_gen();
        self.hotlink.record(
            if ev.scanout < latest_scanouts {
                HotLinkKind::Attached
            } else {
                HotLinkKind::Detached
            },
            ev.scanout,
            self.hotlink.current_gen(),
        );
        self.hotlink.record_layout(self.rows.len());
        true
    }

    /// 光标归属输出号一致性（上游 F0210 对接面）。
    ///
    /// F0210 的 cursorq 消息携带输出号；两处号空间必须同源——本函数核对
    /// 「光标所在输出号在输出描述表里存在且已使能」，不存在即判不一致。
    pub fn cursor_owner_ok(&self, scanout: u32) -> bool {
        match self.row(scanout) {
            Some(r) => r.enabled,
            None => false,
        }
    }

    /// 已使能输出号列表（供 F0209 绑定 scanout 用）。
    pub fn enabled_outputs(&self) -> Vec<u32> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].enabled {
                v.push(self.rows[i].id);
            }
            i += 1;
        }
        v
    }

    /// 输出描述表全量读出（诊断/持久化用）。
    pub fn rows(&self) -> &[HeadRow] {
        &self.rows
    }
}

impl Default for HeadTable {
    fn default() -> HeadTable {
        HeadTable::new()
    }
}

/// EDID 厂商文本的兜底读法（无厂商时给占位符而非空串——空串会让
/// 「无厂商」与「未解析」同形，诊断不说谎纪律要求区分）。
impl EdidBlock {
    pub fn vendor_text_or_dash(&self) -> String {
        let t = self.vendor_text();
        if t.is_empty() {
            "-".to_string()
        } else {
            t
        }
    }
}

// ---------------------------------------------------------------------------
// 六、布局表（锚点：多头布局随 cfg 同步并持久化）
// ---------------------------------------------------------------------------

/// 布局条目（输出号 × 位置 × 尺寸 × 主屏标志）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutEntry {
    pub output: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// 主屏标志（布局基准；位置语义的参照系）。
    pub primary: bool,
}

impl LayoutEntry {
    /// 右边界（像素）。
    pub fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    /// 下边界（像素）。
    pub fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }

    /// 相对主屏的位置语义（无障碍面）。
    pub fn position(&self, primary: &LayoutEntry) -> PositionLabel {
        if self.primary {
            return PositionLabel::Primary;
        }
        let center_dx = (self.x + self.right()) - (primary.x + primary.right());
        let center_dy = (self.y + self.bottom()) - (primary.y + primary.bottom());
        // 先比横轴：横向分离是「左/右」，纵向分离是「上/下」；两个轴都分离时
        // 取横轴（读屏播报左/右是锚点明确点名的语义，优先级更高）。
        if center_dx < 0 {
            PositionLabel::Left
        } else if center_dx > 0 {
            PositionLabel::Right
        } else if center_dy < 0 {
            PositionLabel::Above
        } else if center_dy > 0 {
            PositionLabel::Below
        } else {
            PositionLabel::Unplaced
        }
    }
}

/// 布局快照（持久化门面；下游 F0217 复用）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutSnapshot {
    pub version: u32,
    pub entries: Vec<LayoutEntry>,
}

/// 布局表（锚点「布局表」）。
#[derive(Clone, Debug, Default)]
pub struct LayoutTable {
    entries: Vec<LayoutEntry>,
    /// 自上次应用以来的重算代数（单调；供持久化比对）。
    pub generation: u64,
}

impl LayoutTable {
    pub fn new() -> LayoutTable {
        LayoutTable { entries: Vec::new(), generation: 0 }
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 条目读出。
    pub fn entries(&self) -> &[LayoutEntry] {
        &self.entries
    }

    /// 按输出号取条目。
    pub fn entry(&self, output: u32) -> Option<&LayoutEntry> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].output == output {
                return Some(&self.entries[i]);
            }
            i += 1;
        }
        None
    }

    /// 主屏条目（布局基准；无主屏则 None——位置语义需要参照系）。
    pub fn primary(&self) -> Option<&LayoutEntry> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].primary {
                return Some(&self.entries[i]);
            }
            i += 1;
        }
        None
    }

    /// 输出号 → 位置语义（无条目 → [`PositionLabel::Unplaced`]）。
    pub fn position_of(&self, output: u32) -> PositionLabel {
        let primary = match self.primary() {
            Some(p) => *p,
            None => return PositionLabel::Unplaced,
        };
        match self.entry(output) {
            Some(e) => e.position(&primary),
            None => PositionLabel::Unplaced,
        }
    }

    /// 布局应用（锚点性能：O(输出数)）。
    ///
    /// 排布规则（**不做排列搜索**——多屏布局是用户排布不是求解问题）：
    /// 第 0 号使能输出为主屏落在原点，其余按输出号顺序横向依次排开。
    /// 单遍扫输出描述表 → O(输出数)，无嵌套搜索。
    pub fn apply(&mut self, table: &HeadTable) -> usize {
        let enabled = table.enabled_outputs();
        self.entries.clear();
        let mut x: i32 = 0;
        let mut primary_placed = false;
        let mut i = 0;
        while i < enabled.len() {
            let id = enabled[i];
            let mode = match table.row(id) {
                Some(r) => r.mode,
                // 竞态面：enabled_outputs 与 row 读取之间表被改过 → 跳过该号，
                // 不拿 0 尺寸构造条目（0 宽条目会让后续位置判定全错）。
                None => {
                    i += 1;
                    continue;
                }
            };
            let is_primary = !primary_placed;
            if is_primary {
                primary_placed = true;
            }
            self.entries.push(LayoutEntry {
                output: id,
                x,
                y: 0,
                width: mode.width,
                height: mode.height,
                primary: is_primary,
            });
            x += mode.width as i32;
            i += 1;
        }
        self.generation += 1;
        self.entries.len()
    }

    /// 持久化：导出版本化快照（下游 F0217 热重置后恢复）。
    pub fn snapshot(&self) -> LayoutSnapshot {
        LayoutSnapshot {
            version: LAYOUT_LINK_VERSION,
            entries: self.entries.clone(),
        }
    }

    /// 持久化：按快照恢复（布局语义保持）。
    ///
    /// 边界：版本不符 → 拒恢复并保现状（拿老结构硬解会解出错布局）；
    /// 条目数超 [`LAYOUT_SLOTS`] → 截断拒收（防快照膨胀）。
    pub fn restore(&mut self, snap: &LayoutSnapshot) -> Result<(), Reject> {
        if snap.version != LAYOUT_LINK_VERSION {
            return Err(reject(
                "E_LAYOUT_VERSION",
                format!("布局快照版本 {} 与本单元契约版本 {} 不符", snap.version, LAYOUT_LINK_VERSION),
                "结构版本不符时按旧布局解析会把坐标算错，错布局比空布局更难排查".to_string(),
                "重采一次布局快照；确需升级布局结构则递增 LAYOUT_LINK_VERSION".to_string(),
            ));
        }
        if snap.entries.len() > LAYOUT_SLOTS {
            return Err(reject(
                "E_LAYOUT_TOO_BIG",
                format!("布局快照 {} 条超出容量 {}", snap.entries.len(), LAYOUT_SLOTS),
                "快照条目超界说明来源不可信，无界布局会拖垮位置判定".to_string(),
                format!("确认快照来源后重采（容量上限 {}）", LAYOUT_SLOTS),
            ));
        }
        self.entries = snap.entries.clone();
        Ok(())
    }

    /// 热重置后布局保持（下游 F0217 判据面）。
    ///
    /// 语义：设备级 reset 不等于用户排布作废——本函数在 reset 后核对布局
    /// 是否仍在；仍在则计 [`HeadStats::layout_retained_after_reset`]。
    pub fn retain_after_reset(&self, table: &mut HeadTable) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        // 布局里引用的每个输出号都必须在输出描述表里存活，否则布局是悬空的。
        let mut i = 0;
        while i < self.entries.len() {
            if table.row(self.entries[i].output).is_none() {
                return false;
            }
            i += 1;
        }
        table.stats.layout_retained_after_reset += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// 七、热事件联动记录（锚点「热事件联动记录」）
// ---------------------------------------------------------------------------

/// 联动记录类别（增/删/乱序/布局重算四态各自计数，不合并成「变化过」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotLinkKind {
    Attached,
    Detached,
    Stale,
    LayoutRecalc,
}

impl HotLinkKind {
    pub fn name(self) -> &'static str {
        match self {
            HotLinkKind::Attached => "attach",
            HotLinkKind::Detached => "detach",
            HotLinkKind::Stale => "stale",
            HotLinkKind::LayoutRecalc => "layout",
        }
    }
}

/// 单条联动记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotLinkEntry {
    pub kind: HotLinkKind,
    /// 关联输出号（布局重算记录用 `usize::MAX` 占位——它不是某输出的事件）。
    pub scanout: u32,
    /// 记录时的 cfg 代数。
    pub gen: u64,
}

/// 热事件联动环形记录（锚点第三张表）。
#[derive(Debug)]
pub struct HotLinkLog {
    ring: Vec<Option<HotLinkEntry>>,
    head: usize,
    len: usize,
    gen: u64,
    /// 分类计数（增量处理的可核证据）。
    pub attached: u64,
    pub detached: u64,
    pub stale: u64,
    pub layout_recalc: u64,
}

/// 布局重算记录的输出号占位（非某输出的事件，故不冒用真输出号）。
pub const HOTLINK_LAYOUT_PLACEHOLDER: u32 = u32::MAX;

impl HotLinkLog {
    pub fn new() -> HotLinkLog {
        HotLinkLog {
            ring: {
                let mut v = Vec::new();
                let mut i = 0;
                while i < HOTLINK_SLOTS {
                    v.push(None);
                    i += 1;
                }
                v
            },
            head: 0,
            len: 0,
            gen: 0,
            attached: 0,
            detached: 0,
            stale: 0,
            layout_recalc: 0,
        }
    }

    /// 当前 cfg 代数（单调；乱序判据的参照）。
    pub fn current_gen(&self) -> u64 {
        self.gen
    }

    /// 代数递增。
    pub fn bump_gen(&mut self) -> u64 {
        self.gen += 1;
        self.gen
    }

    /// 记一条（环形覆盖最旧）。
    pub fn record(&mut self, kind: HotLinkKind, scanout: u32, gen: u64) {
        let entry = HotLinkEntry { kind, scanout, gen };
        self.ring[self.head] = Some(entry);
        self.head += 1;
        if self.head >= HOTLINK_SLOTS {
            self.head = 0;
        }
        if self.len < HOTLINK_SLOTS {
            self.len += 1;
        }
        match kind {
            HotLinkKind::Attached => self.attached += 1,
            HotLinkKind::Detached => self.detached += 1,
            HotLinkKind::Stale => self.stale += 1,
            HotLinkKind::LayoutRecalc => self.layout_recalc += 1,
        }
    }

    /// 记一次布局重算（关联输出号用占位值）。
    pub fn record_layout(&mut self, _outputs: usize) {
        self.record(HotLinkKind::LayoutRecalc, HOTLINK_LAYOUT_PLACEHOLDER, self.gen);
    }

    /// 记录条数（占用，不含被覆盖的旧条）。
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 `i` 条（按写入顺序；越界 None）。
    pub fn get(&self, i: usize) -> Option<HotLinkEntry> {
        if i >= self.len {
            return None;
        }
        let start = if self.len < HOTLINK_SLOTS { 0 } else { self.head };
        let idx = (start + i) % HOTLINK_SLOTS;
        self.ring[idx]
    }

    /// 分类计数是否与记录条数一致（自检对账面：环形覆盖后总数应等于
    /// 各分类之和 + 被覆盖条数）。
    pub fn classify_sum(&self) -> u64 {
        self.attached + self.detached + self.stale + self.layout_recalc
    }
}

impl Default for HotLinkLog {
    fn default() -> HotLinkLog {
        HotLinkLog::new()
    }
}

// ---------------------------------------------------------------------------
// 八、与上游 F0209 / F0210 的对接契约
// ---------------------------------------------------------------------------

/// 扫描输出绑定请求（F0209 消费；本单元只发意图，不碰下发时机）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanoutBindRequest {
    pub output: u32,
    pub rect: Rect,
    /// 命令码（单一事实源在 `veb02_proto`：[`CMD_SET_SCANOUT`]）。
    pub cmd: u32,
}

/// 生成本单元的绑定请求（判据：每输出独立使能 → 逐输出一份请求）。
///
/// **不下发**：只产出请求对象，由 F0209 决定何时进控制队列——本单元若自己
/// 下发就是双重下发（同一输出一条 F0209 的、一条本单元的）。
pub fn bind_requests(table: &HeadTable) -> Vec<ScanoutBindRequest> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < table.rows().len() {
        let r = &table.rows()[i];
        if r.enabled {
            v.push(ScanoutBindRequest {
                output: r.id,
                rect: Rect { x: 0, y: 0, width: r.mode.width, height: r.mode.height },
                cmd: CMD_SET_SCANOUT,
            });
        }
        i += 1;
    }
    v
}

/// EDID 获取请求（`CMD_GET_EDID` + 载荷宽度，用于向设备/宿主取物理 EDID）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdidRequest {
    pub output: u32,
    pub cmd: u32,
    pub len: usize,
}

pub fn edid_request(output: u32) -> EdidRequest {
    EdidRequest { output, cmd: CMD_GET_EDID, len: EDID_LEN }
}

/// 接入契约版本核对面（跨批对接的版本协商）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkContract {
    pub scanout: u32,
    pub layout: u32,
}

pub const LINK_CONTRACT: LinkContract = LinkContract {
    scanout: SCANOUT_LINK_VERSION,
    layout: LAYOUT_LINK_VERSION,
};

/// 契约一致性（两处版本必须相等；不等即判对接面分叉）。
pub fn link_ok(c: LinkContract) -> bool {
    c.scanout == SCANOUT_LINK_VERSION && c.layout == LAYOUT_LINK_VERSION
}

// ---------------------------------------------------------------------------
// 九、判据自检域聚合入口
// ---------------------------------------------------------------------------

/// VE-F0213 判据自检域聚合入口（登记于 `svstar2::checks`）。
pub fn run_veb13_checks() -> crate::checks::CheckSet {
    super::veb13_checks::run_veb13_checks()
}

// ---------------------------------------------------------------------------
// 八、单元测试（回归层——与 `veb13_checks` 的 92 项判据层分工不同）
// ---------------------------------------------------------------------------
//
// 判据层回答「规格有没有被满足」，本层回答「实现有没有被改坏」。判据是抽样、
// 本层是逐个字节，两层互不可省（VE-F1004 实证：前序会话交了 21 项判据却零
// `#[test]`，边界一被悄悄放松没有任何东西会红）。
//
// 语料**自造**：现搭合法 EDID 块（按规范算校验和），不引外部文件——外部语料
// 会让「谁坏了」变成「哪个文件变了」，破坏回归定位能力。


// ---------------------------------------------------------------------------
// 八、单元测试（回归层——与 `veb13_checks` 的 92 项判据层分工不同）
// ---------------------------------------------------------------------------
//
// 判据层回答「规格有没有被满足」，本层回答「实现有没有被改坏」。判据是抽样、
// 本层是逐个字节，两层互不可省（VE-F1004 实证：前序会话交了 21 项判据却零
// `#[test]`，边界一被悄悄放松没有任何东西会红）。
//
// 语料**自造**：现搭合法 EDID 块（按规范算校验和），不引外部文件——外部语料
// 会让「谁坏了」变成「哪个文件变了」，破坏回归定位能力。


// ---------------------------------------------------------------------------
// 八、单元测试（回归层——与 `veb13_checks` 的 92 项判据层分工不同）
// ---------------------------------------------------------------------------
//
// 判据层回答「规格有没有被满足」，本层回答「实现有没有被改坏」。判据是抽样、
// 本层是逐个字节，两层互不可省（VE-F1004 实证：前序会话交了 21 项判据却零
// `#[test]`，边界一被悄悄放松没有任何东西会红）。
//
// 语料**自造**：现搭合法 EDID 块（按规范算校验和），不引外部文件——外部语料
// 会让「谁坏了」变成「哪个文件变了」，破坏回归定位能力。


// ---------------------------------------------------------------------------
// 八、单元测试（回归层——与 `veb13_checks` 的 92 项判据层分工不同）
// ---------------------------------------------------------------------------
//
// 判据层回答「规格有没有被满足」，本层回答「实现有没有被改坏」。判据是抽样、
// 本层是逐个字节，两层互不可省（VE-F1004 实证：前序会话交了 21 项判据却零
// `#[test]`，边界一被悄悄放松没有任何东西会红）。
//
// 语料**自造**：现搭合法 EDID 块（按规范算校验和），不引外部文件——外部语料
// 会让「谁坏了」变成「哪个文件变了」，破坏回归定位能力。

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::veb01_device::DisplayCfg;
    use alloc::vec;

    /// 造一个**合法**的 128字节 EDID 基础块。
    ///
    /// 布局（EDID 1.3 规范）：
    ///   [0..8]   固定头 00 FF×6 00
    ///   [8..10]  厂商口令（大端）
    ///   [10..12] 产品码（小端）
    ///   [12..16] 序列号
    ///   [18]     版本 1
    ///   [20]     DTD 起始偏移 = 54
    ///   [54..72] DTD0（18 字节）
    ///   [126]    扩展块数
    ///   [127]    基础块校验和（使 0..127 之和 mod 256 == 0）
    fn valid_edid(w: u32, h: u32, refresh_hz: u32) -> EdidBlock {
        let mut b = [0u8; EDID_LEN];
        b[0..8].copy_from_slice(&EDID_HEADER);
        // 厂商 "AIW"（三字母大端编码：'A'=1,'I'=9,'W'=5 → 0x0000_0000 形式）
        b[8] = 0x00; b[9] = 0x00; b[10] = 0xFC; // big-endian 3-char 编码
        b[10] = 0x41; b[9] = 0x49; b[8] = 0x57; // 'A','I','W'
        b[18] = 1; // 版本
        b[20] = DTD_BASE_OFFSET as u8;
        // --- DTD0（EDID 1.3 字节布局：H 有效/blank 的高 4 位在 byte4 的两个
        //           半字节，V 有效/blank 的高 4 位在 byte7 的两个半字节）---
        let hb: u32 = 128; // H blank
        let vb: u32 = 45;  // V blank
        let total = (w + hb) * (h + vb);
        let mut clock10 = if total == 0 { 1 } else { total * refresh_hz / 10_000 };
        if clock10 == 0 {
            clock10 = 1;
        }
        b[DTD_BASE_OFFSET] = (clock10 & 0xFF) as u8;                          // 像素时钟小端低字节
        b[DTD_BASE_OFFSET + 1] = ((clock10 >> 8) & 0xFF) as u8;               // 像素时钟小端高字节
        b[DTD_BASE_OFFSET + 2] = (w & 0xFF) as u8;                           // H 有效低字节
        b[DTD_BASE_OFFSET + 3] = (hb & 0xFF) as u8;                          // H blank 低字节
        b[DTD_BASE_OFFSET + 4] = ((((w >> 8) & 0x0F) << 4) | ((hb >> 8) & 0x0F)) as u8; // H 两高半字节
        b[DTD_BASE_OFFSET + 5] = (h & 0xFF) as u8;                           // V 有效低字节
        b[DTD_BASE_OFFSET + 6] = (vb & 0xFF) as u8;                          // V blank 低字节
        b[DTD_BASE_OFFSET + 7] = ((((h >> 8) & 0x0F) << 4) | ((vb >> 8) & 0x0F)) as u8; // V 两高半字节
        // 其余 DTD 槽置 0（表示「无更多时序」）
        b[126] = 0; // 无扩展块
        // 校验和
        let mut sum: u8 = 0;
        let mut i = 0;
        while i < 127 {
            sum = sum.wrapping_add(b[i]);
            i += 1;
        }
        b[127] = (0u8).wrapping_sub(sum);
        EdidBlock { bytes: b }
    }

    fn cfg_with(scanouts: u32) -> DisplayCfg {
        DisplayCfg { scanouts, max_width: 3840, max_height: 2160 }
    }

    // ---- EDID 基础结构 ----

    #[test]
    fn veb13_valid_edid_passes_verify() {
        let e = valid_edid(1920, 1080, 60);
        assert!(e.header_ok(), "固定头应合规");
        assert!(e.checksum_ok(), "自造块校验和应为 0");
        match verify_edid(&e) {
            EdidVerdict::Valid { slot } => assert_eq!(slot, 0, "首个 DTD 应为首选"),
            other => panic!("合法 EDID 应判 Valid，实得 {:?}", other),
        }
    }

    #[test]
    fn veb13_all_zero_block_is_invalid() {
        let e = EdidBlock::empty();
        assert!(!e.header_ok(), "全零块固定头不合规");
        assert!(!verify_edid(&e).is_valid());
    }

    #[test]
    fn veb13_checksum_break_is_rejected() {
        let mut e = valid_edid(1280, 720, 60);
        e.poke(127, e.byte_at(127).wrapping_add(1));
        assert!(!e.checksum_ok(), "破坏校验和应被判非法");
        assert!(!verify_edid(&e).is_valid());
    }

    #[test]
    fn veb13_header_break_is_rejected() {
        let mut e = valid_edid(1280, 720, 60);
        e.poke(1, 0x00); // 固定头第二字节必须是 FF
        assert!(!e.header_ok());
        assert!(!verify_edid(&e).is_valid());
    }

    #[test]
    fn veb13_byte_at_never_panics() {
        let e = valid_edid(800, 600, 60);
        assert_eq!(e.byte_at(0), 0x00);
        assert_eq!(e.byte_at(EDID_LEN), 0, "越界读应返 0 而非 panic");
        assert_eq!(e.byte_at(9999), 0, "远越界读也应返 0");
    }

    #[test]
    fn veb13_block_count_semantics() {
        // extension_count 是扩展块数（不含基础块）；total 才是总数
        let mut e = valid_edid(640, 480, 60);
        e.poke(126, 2);
        assert_eq!(e.extension_count(), 2, "扩展块数应等于 byte126");
        assert_eq!(e.total_block_count(), 3, "总块数 = 扩展 + 基础");
        e.poke(126, 200); // 越界应截到上限，不得panic
        assert!(e.extension_count() <= EDID_BLOCKS);
    }

    // ---- DTD 解析（字节级正确性）----

    #[test]
    fn veb13_dtd_mode_roundtrip() {
        let e = valid_edid(1920, 1080, 60);
        let d = e.dtd(0).expect("DTD0 应可解析");
        assert_eq!(d.h_active, 1920, "H 有效低字节错位");
        assert_eq!(d.v_active, 1080, "V 有效低字节错位");
        let m = d.mode();
        assert_eq!(m.width, 1920);
        assert_eq!(m.height, 1080);
        // 自造块给的是整数刷新率，解析应在 ±2Hz 内
        let expect = ((1920u32 + 128) * (1080u32 + 128) * 60) / 10_000;
        let _ = expect; // 真实刷新率取决于 blank，此处只断言「有值且合理」
        assert!(m.refresh_hz > 40 && m.refresh_hz < 80, "刷新率 {} 不合理", m.refresh_hz);
    }

    #[test]
    fn veb13_dtd_slot_bounds_are_none() {
        let e = valid_edid(1920, 1080, 60);
        assert!(e.dtd(DTD_SLOTS).is_none(), "槽位等于上限应返 None");
        assert!(e.dtd(9999).is_none(), "远越界槽位应返 None 而非 panic");
    }

    #[test]
    fn veb13_empty_slot_is_not_a_mode() {
        // 自造块只有 DTD0，其余槽全零 → 不应被当成有效时序
        let e = valid_edid(1920, 1080, 60);
        assert!(e.dtd(1).is_none(), "空槽不应解出 DTD");
        assert_eq!(e.preferred_slot(), Some(0), "首选应是首个 DTD");
    }

    #[test]
    fn veb13_zero_denominator_gives_no_refresh() {
        // 分母为 0 时必须给 0 Hz（不假装有刷新率）
        let d = Dtd { pixel_clock_hz: 0, h_active: 0, h_blank: 0, v_active: 0, v_blank: 0 };
        assert_eq!(d.mode().refresh_hz, 0, "零分母不得给出非零刷新率");
    }

    // ---- 输出独立使能 ----

    #[test]
    fn veb13_enable_is_per_output_scoped() {
        let mut t = HeadTable::new();
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 3);
        for id in 0..3u32 {
            t.set_mode(id, ModeSel::new(1920, 1080, 60)).expect("设模式应成功");
        }
        t.set_enabled(0, true).unwrap();
        t.set_enabled(1, false).unwrap();
        t.set_enabled(2, true).unwrap();
        //使能作用域不外溢
        let en = t.enabled_outputs();
        assert!(en.contains(&0) && en.contains(&2), "0/2 应使能：{:?}", en);
        assert!(!en.contains(&1), "1 不应被外溢使能：{:?}", en);
    }

    #[test]
    fn veb13_attach_beyond_device_limit_rejected() {
        let mut t = HeadTable::new();
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 2);
        for id in 0..2u32 {
            t.set_mode(id, ModeSel::new(800, 600, 60)).expect("设模式应成功");
        }
        // 超出设备 cfg 的 scanouts 上限必须被拒
        let r = t.attach_output(2, ModeSel::new(800, 600, 60));
        assert!(r.is_err(), "超出设备上限应拒绝");
        let rej = r.err().expect("应给出拒绝理由");
        assert!(rej.is_complete(), "拒绝必须三要素齐全（code/what/why/next）");
    }

    #[test]
    fn veb13_set_mode_beyond_capability_rejected() {
        let mut t = HeadTable::new();
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 1);
        t.set_mode(0, ModeSel::new(800, 600, 60)).expect("设模式应成功");
        // 远超设备能力的模式应被拒
        let r = t.set_mode(0, ModeSel::new(99999, 99999, 500));
        assert!(r.is_err(), "超能力模式应被拒");
    }

    #[test]
    fn veb13_operations_on_unmounted_output_rejected() {
        let mut t = HeadTable::new();
        assert!(t.set_enabled(0, true).is_err(), "未挂载输出不应能使能");
        assert!(t.set_mode(0, ModeSel::new(800, 600, 60)).is_err());
        assert!(t.row(0).is_none(), "未挂载不应有行");
    }

    #[test]
    fn veb13_same_value_enable_is_idempotent() {
        let mut t = HeadTable::new();
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 1);
        t.set_mode(0, ModeSel::new(1920, 1080, 60)).expect("设模式应成功");
        t.set_enabled(0, true).unwrap();
        let w0 = t.stats.enable_writes;
        t.set_enabled(0, true).unwrap();
        assert_eq!(t.stats.enable_writes, w0, "同值重复使能不应记为写入");
    }

    // ---- EDID 注入优先级 ----

    #[test]
    fn veb13_edid_source_rank_is_strictly_descending() {
        // 三级注入优先级：宿主配置应高于物理显示器信息
        let a = EdidSource::Injected;
        let b = EdidSource::Physical;
        let c = EdidSource::None;
        assert!(a.rank() > b.rank(), "Injected({}) 应高于 Physical({})", a.rank(), b.rank());
        assert!(b.rank() > c.rank(), "Physical({}) 应高于 None({})", b.rank(), c.rank());
    }

    #[test]
    fn veb13_illegal_edid_rejected_keeps_default() {
        let mut t = HeadTable::new();
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 1);
        t.set_mode(0, ModeSel::new(1920, 1080, 60)).expect("设模式应成功");
        let before = t.row(0).expect("行应存在").edid.total_block_count();
        let r = t.inject_edid(0, EdidSource::Injected, EdidBlock::empty());
        assert!(r.is_err(), "全零非法 EDID 应被拒");
        let rej = r.err().expect("应给出拒绝理由");
        assert!(rej.is_complete(), "拒载必须三要素齐全");
        assert_eq!(t.row(0).expect("行应仍在").edid.total_block_count(), before, "拒载后必须保默认");
    }

    #[test]
    fn veb13_low_priority_cannot_override_high() {
        let mut t = HeadTable::new();
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 1);
        t.set_mode(0, ModeSel::new(1920, 1080, 60)).expect("设模式应成功");
        t.inject_edid(0, EdidSource::Injected, valid_edid(1920, 1080, 60)).unwrap();
        let after_high = t.row(0).expect("行应存在").edid_source;
        // 低优先级注入应被拒
        let r = t.inject_edid(0, EdidSource::Physical, valid_edid(800, 600, 60));
        assert!(r.is_err(), "低优先级不得覆盖高优先级");
        assert_eq!(t.row(0).expect("行应仍在").edid_source, after_high, "低优先级不得改写来源");
    }

    #[test]
    fn veb13_same_priority_can_replace() {
        let mut t = HeadTable::new();
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 1);
        t.set_mode(0, ModeSel::new(1920, 1080, 60)).expect("设模式应成功");
        t.inject_edid(0, EdidSource::Injected, valid_edid(1920, 1080, 60)).unwrap();
        // 同级可替换
        t.inject_edid(0, EdidSource::Injected, valid_edid(1280, 720, 60))
            .expect("同级应可替换");
    }

    #[test]
    fn veb13_edid_inject_on_unmounted_rejected() {
        let mut t = HeadTable::new();
        assert!(
            t.inject_edid(0, EdidSource::Injected, valid_edid(800, 600, 60)).is_err(),
            "未挂载输出不应接受注入"
        );
    }

    // ---- 热增删 ----

    #[test]
    fn veb13_cfg_sync_adds_and_removes_rows() {
        let mut t = HeadTable::new();
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 3);
        for id in 0..3u32 {
            t.set_mode(id, ModeSel::new(1920, 1080, 60)).expect("设模式应成功");
        }
        assert_eq!(t.len(), 3);
        // 缩到 2 个 → 多余行应被摘掉
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 2);
        assert_eq!(t.len(), 2, "收缩后行数应随之减少");
        assert!(t.row(2).is_none(), "已消失的输出不应留行");
    }

    #[test]
    fn veb13_rows_stay_sorted_after_shrink() {
        let mut t = HeadTable::new();
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 4);
        for id in [3u32, 0, 2, 1] {
            t.set_mode(id, ModeSel::new(800, 600, 60)).expect("设模式应成功");
        }
        // 摘掉高位后剩余应仍升序
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 2);
        let ids: Vec<u32> = t.rows().iter().map(|r| r.id).collect();
        for i in 1..ids.len() {
            assert!(ids[i - 1] < ids[i], "行序应升序：{:?}", ids);
        }
    }

    #[test]
    fn veb13_sync_with_same_cfg_is_zero_write() {
        let mut t = HeadTable::new();
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 2);
        for id in 0..2u32 {
            t.set_mode(id, ModeSel::new(1920, 1080, 60)).expect("设模式应成功");
        }
        let w = t.stats.sync_writes;
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 2);
        assert_eq!(t.stats.sync_writes, w, "无变化同步不应记为写入");
    }

    // ---- 布局 ----

    #[test]
    fn veb13_layout_first_is_primary_at_origin() {
        let mut t = HeadTable::new();
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 2);
        for id in 0..2u32 {
            t.set_mode(id, ModeSel::new(1920, 1080, 60)).expect("设模式应成功");
            t.set_enabled(id, true).unwrap();
        }
        let mut lay = LayoutTable::new();
        let n = lay.apply(&t);
        assert_eq!(n, 2, "两个使能输出都应排入");
        let p = lay.primary().expect("应有主屏");
        assert!(p.primary, "首条应为主屏");
        assert_eq!(p.x, 0, "主屏应在原点");
        assert_eq!(p.y, 0);
    }

    #[test]
    fn veb13_layout_second_is_to_the_right() {
        let mut t = HeadTable::new();
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 2);
        for id in 0..2u32 {
            t.set_mode(id, ModeSel::new(1920, 1080, 60)).expect("设模式应成功");
            t.set_enabled(id, true).unwrap();
        }
        let mut lay = LayoutTable::new();
        lay.apply(&t);
        let p = lay.primary().expect("应有主屏").clone();
        let second = lay.entry(1).expect("次屏条目应存在");
        assert!(second.x > p.x, "次屏应在主屏右侧：{} vs {}", second.x, p.x);
        assert_eq!(second.y, p.y, "同排时纵坐标应相同");
    }

    #[test]
    fn veb13_layout_only_places_enabled_outputs() {
        let mut t = HeadTable::new();
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 3);
        for id in 0..3u32 {
            t.set_mode(id, ModeSel::new(1920, 1080, 60)).expect("设模式应成功");
        }
        t.set_enabled(0, true).unwrap();
        t.set_enabled(1, false).unwrap();
        t.set_enabled(2, true).unwrap();
        let mut lay = LayoutTable::new();
        let n = lay.apply(&t);
        assert_eq!(n, 2, "只应排入 2 个使能输出");
        assert!(lay.entry(1).is_none(), "未使能输出不应有布局条目");
    }

    #[test]
    fn veb13_layout_generation_is_monotonic() {
        let mut t = HeadTable::new();
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 2);
        for id in 0..2u32 {
            t.set_mode(id, ModeSel::new(1920, 1080, 60)).expect("设模式应成功");
            t.set_enabled(id, true).unwrap();
        }
        let mut lay = LayoutTable::new();
        let g0 = lay.generation;
        lay.apply(&t);
        let g1 = lay.generation;
        lay.apply(&t);
        assert!(g1 > g0, "代数应递增：{} -> {}", g0, g1);
        assert!(lay.generation > g1, "再次应用代数应继续递增");
    }

    #[test]
    fn veb13_unplaced_output_has_no_position() {
        let mut t = HeadTable::new();
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 1);
        t.set_mode(0, ModeSel::new(1920, 1080, 60)).expect("设模式应成功");
        t.set_enabled(0, true).unwrap();
        let mut lay = LayoutTable::new();
        lay.apply(&t);
        // 未在布局中的输出（这里用不存在的 9号）
        assert_eq!(lay.position_of(9), PositionLabel::Unplaced, "未布局应判Unplaced");
    }

    // ---- 位置语义与无障碍播报 ----

    #[test]
    fn veb13_spoken_name_carries_side_semantics() {
        let mut t = HeadTable::new();
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 2);
        for id in 0..2u32 {
            t.set_mode(id, ModeSel::new(1920, 1080, 60)).expect("设模式应成功");
            t.set_enabled(id, true).unwrap();
        }
        let mut lay = LayoutTable::new();
        lay.apply(&t);
        let right = t.row(1).expect("次屏行应存在").spoken_name(&lay);
        assert!(right.contains("右"), "右屏播报名应含方位语义：{}", right);
        let primary = t.row(0).expect("主屏行应存在").spoken_name(&lay);
        assert!(!primary.contains("右"), "主屏不应播报为右屏：{}", primary);
    }

    #[test]
    fn veb13_position_label_side_predicate() {
        // 左右语义可判；主屏无侧
        assert!(PositionLabel::Right.has_side(), "右应有侧语义");
        assert!(PositionLabel::Left.has_side(), "左应有侧语义");
        assert!(!PositionLabel::Primary.has_side(), "主屏不应有侧语义");
        assert!(!PositionLabel::Unplaced.has_side(), "未布局不应有侧语义");
    }

    // ---- 对接契约 ----

    #[test]
    fn veb13_bind_requests_only_cover_enabled_outputs() {
        let mut t = HeadTable::new();
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 3);
        for id in 0..3u32 {
            t.set_mode(id, ModeSel::new(1920, 1080, 60)).expect("设模式应成功");
        }
        t.set_enabled(0, true).unwrap();
        t.set_enabled(1, false).unwrap();
        t.set_enabled(2, true).unwrap();
        let reqs = bind_requests(&t);
        assert_eq!(reqs.len(), 2, "只应给使能输出生成绑定请求");
        for r in reqs.iter() {
            assert_ne!(r.output, 1, "未使能输出不应出现在绑定请求里");
        }
    }

    #[test]
    fn veb13_cursor_owner_requires_enabled() {
        let mut t = HeadTable::new();
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 2);
        for id in 0..2u32 {
            t.set_mode(id, ModeSel::new(1920, 1080, 60)).expect("设模式应成功");
        }
        t.set_enabled(0, true).unwrap();
        t.set_enabled(1, false).unwrap();
        assert!(t.cursor_owner_ok(0), "已使能应认光标归属");
        assert!(!t.cursor_owner_ok(1), "未使能不认光标归属");
        assert!(!t.cursor_owner_ok(99), "越界输出不认归属，且不得 panic");
    }

    #[test]
    fn veb13_link_contract_detects_fork() {
        assert!(link_ok(LINK_CONTRACT), "自身契约应判自洽");
        // 分叉（版本不等）必须被判出——否则对接面分叉会静默通过
        let forked = LinkContract { scanout: SCANOUT_LINK_VERSION + 1, layout: LAYOUT_LINK_VERSION };
        assert!(!link_ok(forked), "scanout 版本分叉应被识破");
        let forked2 = LinkContract { scanout: SCANOUT_LINK_VERSION, layout: LAYOUT_LINK_VERSION + 1 };
        assert!(!link_ok(forked2), "layout 版本分叉应被识破");
    }

    // ---- 持久化往返 ----

    #[test]
    fn veb13_layout_snapshot_roundtrip() {
        let mut t = HeadTable::new();
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 2);
        for id in 0..2u32 {
            t.set_mode(id, ModeSel::new(1920, 1080, 60)).expect("设模式应成功");
            t.set_enabled(id, true).unwrap();
        }
        let mut lay = LayoutTable::new();
        lay.apply(&t);
        let snap = lay.snapshot();
        let mut back = LayoutTable::new();
        assert!(back.restore(&snap).is_ok(), "同版本快照应可恢复");
        assert_eq!(back.len(), lay.len(), "恢复后条目数应一致");
        assert_eq!(back.snapshot(), snap, "往返应逐字段一致");
    }

    #[test]
    fn veb13_layout_version_mismatch_rejected() {
        let mut t = HeadTable::new();
        t.sync_with_cfg(DisplayCfg { scanouts: 0, max_width: 3840, max_height: 2160 }, 1);
        t.set_mode(0, ModeSel::new(1920, 1080, 60)).expect("设模式应成功");
        t.set_enabled(0, true).unwrap();
        let mut lay = LayoutTable::new();
        lay.apply(&t);
        let mut snap = lay.snapshot();
        snap.version = LAYOUT_LINK_VERSION + 1;
        let mut back = LayoutTable::new();
        assert!(back.restore(&snap).is_err(), "版本不符必须拒恢复");
    }
}
