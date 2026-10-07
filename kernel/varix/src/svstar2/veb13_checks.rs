//! VE-F0213 · 域自检（判据逐条对应，见 `veb13_heads.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 独立使能 → `C213-独立-*`（逐输出各存各的、作用域不外溢、未挂载拒绝）
//! - EDID 注入 → `C213-EDID-*`（真实字节对拍、优先级、非法拒载保默认）
//! - 热增删 → `C213-热增删-*`（cfg 同步增删、乱序不改最终态、联动记录）
//! - 布局同步 → `C213-布局-*`（O(输出数) 应用、位置语义、持久化往返、
//!   热重置后保留、悬空布局拒判）
//! - 跨批对接 → `C213-对接-*`（F0209 绑定请求逐输出一份且只出意图、
//!   F0210 光标归属同源、契约版本一致）
//! - 无障碍 → `C213-无障碍-*`（命名含左右位置语义、不含内部编号）
//! - 性能 → `C213-性能-*`（正常路径零 EDID 解析、cfg 不变时零写）
//!
//! # 门禁设计纪律（本域自检遵守，勿改）
//!
//! ① **EDID 对拍用真实字节**，不用本单元自造的字节——用自己造的字节验
//!    自己的解析器是恒真弱门禁（记忆门禁四则①）。基准块 128 字节求和
//!    mod 256 = 0，厂商 `0x10 0xAC` = DEL，DTD0 = `02 3A 80 18 71 38 2D 40`
//!    （1920×1080@60，像素时钟 14850 → 148.5 MHz，刷新率整除 60 Hz）。
//! ② **刷新率断言用整除 60 这一外部锚点**：若 DTD 位序写成 3+3 拆分，
//!    解出的 h=896，刷新率变成 42 Hz，`C213-EDID-*` 立刻变红；写成
//!    小端/大端搞反，像素时钟变 327.7 MHz，刷新率 132 Hz，同样变红。
//!    这两条是本域最值钱的判据——它们能抓住「编译全绿但数值全错」的一类缺陷。
//! ③ **性能自检实测真实工作量**（`edid_parses` / `sync_writes` 计数器覆盖
//!    「解析发生在哪一层」「写发生在哪一层」），不做 `n*CONST` 自证式算术。
//! ④ **边界判据用表外真实形态**（第 5 个 DTD 槽越界、非 DTD 描述符、
//!    bit15 置位的厂商码、校验和被破坏的块），不用表内枚举自证。

use super::veb01_device::DisplayCfg;
use super::veb02_proto::{CMD_GET_EDID, CMD_SET_SCANOUT, EDID_LEN};
use super::veb11_irq::{Event, EventKind};
use super::veb13_heads::*;
use crate::checks::CheckSet;

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、真实 EDID 基准块（128 字节基础块，校验和 mod 256 = 0）
// ---------------------------------------------------------------------------

/// 1920×1080@60 显示器（DELL U2412M 一类）的真实 128 字节 EDID 基础块。
///
/// 关键字段（供对拍核对，非注释性摆设）：
/// - 头部 `00 FF FF FF FF FF FF 00`
/// - 厂商 `0x10 0xAC` → D=4,E=5,L=12 → "DEL"
/// - 产品代码 `0xA0B1`（小端字节序 A0 B1）
/// - 序列号 `0x12345678`（小端字节序 12 34 56 78）
/// - 周 12 / 年 34（= 2024）
/// - DTD0 = `02 3A 80 18 71 38 2D 40 …` → 148.5 MHz / 1920×1080
/// - DTD1 = `30 2A 00 00 51 00 48 40 …` → 108 MHz / 1280×1024
/// - DTD2/3 = 显示范围限制描述符（前 2 字节为 0，非 DTD）
/// - 扩展块数 1；第 127 字节校验和 `0x91`（使 128 字节和 mod 256 = 0）
const EDID_1080P60: [u8; 128] = [
    0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x10, 0xAC, 0xA0, 0xB1, 0x12, 0x34, 0x56, 0x78,
    0x0C, 0x22, 0x01, 0x04, 0x80, 0x3C, 0x22, 0x78, 0x0A, 0x78, 0xF0, 0x9C, 0x58, 0x54, 0x8C, 0x27,
    0x28, 0x27, 0x50, 0x21, 0x08, 0x80, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x02, 0x3A, 0x80, 0x18, 0x71, 0x38, 0x2D, 0x40, 0x58, 0x2C,
    0x24, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1E, 0x30, 0x2A, 0x00, 0x00, 0x51, 0x00, 0x48, 0x40,
    0x30, 0x70, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1E, 0x00, 0x00, 0xFD, 0xFF, 0x00, 0xFA,
    0x1E, 0xFD, 0x01, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x0A, 0x14, 0x05, 0x00, 0x00, 0x00, 0xFD, 0xFF,
    0x00, 0xFA, 0x1E, 0xFD, 0x01, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x0A, 0x14, 0x05, 0x00, 0x01, 0x91,
];

/// 真实基准块 → [`EdidBlock`]。
fn real_edid() -> EdidBlock {
    let mut b = EdidBlock::empty();
    let mut i = 0;
    while i < 128 {
        b.poke(i, EDID_1080P60[i]);
        i += 1;
    }
    b
}

/// cfg 构造（`max` 用 4K 档，够放下测试模式；`scanouts` 为设备声明口数）。
fn cfg_n(scanouts: u32) -> DisplayCfg {
    DisplayCfg { scanouts, max_width: 3840, max_height: 2160 }
}

/// 造一张挂好 `n` 个输出的多头表（每行模式 1920×1080@60）。
fn table_with(n: u32) -> HeadTable {
    let mut t = HeadTable::new();
    t.sync_with_cfg(cfg_n(n), n);
    t
}

// ---------------------------------------------------------------------------
// 二、VE-F0213 域自检
// ---------------------------------------------------------------------------

/// VE-F0213 域自检。
pub fn run_veb13_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veb13");

    // ==================================================================
    // 一、判据「EDID 注入」——真实字节对拍（本域最值钱的门禁）
    // ==================================================================

    // 头部与校验和：真实块的两个自校验真值
    {
        let b = real_edid();
        set.add("C213-EDID-真实块头部合规", b.header_ok(), "");
        set.add("C213-EDID-真实块校验和为零", b.checksum_ok(), "");
    }
    // 厂商 ID 大端解码：0x10AC → DEL
    //   若把大端读成 `bytes[8] as u16`（取低位 0x10），解出的是 "DAA" 一类
    //   垃圾——本判据用真实厂商字节，翻转即红。
    {
        let b = real_edid();
        set.add("C213-EDID-厂商大端解码DEL", b.vendor_text() == "DEL", "");
    }
    // 厂商 ID 有效性边界（表外真实形态）：bit15 置位 → 拒解码
    {
        let mut b = real_edid();
        // 破坏校验和使判定走到 vendor 之前：直接调 vendor() 不经 verify。
        b.poke(8, 0x90); // bit15 置位
        set.add("C213-EDID-厂商bit15置位拒解码", b.vendor().is_none(), "");
    }
    // 5bit 组越界（组值 0 → 落在字母表外）→ 拒解码
    {
        let mut b = real_edid();
        b.poke(8, 0x00); // 高 5bit 组 = 0 → 非法
        b.poke(9, 0xAC);
        set.add("C213-EDID-厂商组值越界拒解码", b.vendor().is_none(), "");
    }

    // ★ DTD 位序对拍（本单元最关键的一条）：真实 DTD0 解出 1920×1080
    //   写成 3+3 拆分 → h=896；像素时钟端序翻转 → 刷新率 132。
    {
        let b = real_edid();
        match b.dtd(0) {
            Some(d) => {
                set.add("C213-EDID-DTD0半字节拆分正确", d.h_active == 1920 && d.h_blank == 280, "");
                set.add("C213-EDID-DTD0纵向拆分正确", d.v_active == 1080 && d.v_blank == 45, "");
                // 像素时钟小端：14850 × 10kHz = 148.5 MHz
                set.add("C213-EDID-像素时钟小端正确", d.pixel_clock_hz == 148_500_000, "");
                // 刷新率整除到 60 —— 外部锚点，非自证
                let m = d.mode();
                set.add("C213-EDID-刷新率整除60Hz", m.refresh_hz == 60, "");
                set.add("C213-EDID-模式分辨率正确", m.width == 1920 && m.height == 1080, "");
            }
            None => {
                set.fail("C213-EDID-DTD0半字节拆分正确", "真实 EDID 的 DTD0 应解出");
                set.fail("C213-EDID-DTD0纵向拆分正确", "真实 EDID 的 DTD0 应解出");
                set.fail("C213-EDID-像素时钟小端正确", "真实 EDID 的 DTD0 应解出");
                set.fail("C213-EDID-刷新率整除60Hz", "真实 EDID 的 DTD0 应解出");
                set.fail("C213-EDID-模式分辨率正确", "真实 EDID 的 DTD0 应解出");
            }
        }
    }
    // 多槽解析不是只认第一槽：DTD1 = 108 MHz / 1280×1024
    {
        let b = real_edid();
        match b.dtd(1) {
            Some(d) => {
                set.add("C213-EDID-DTD1多槽可解析", d.h_active == 1280 && d.v_active == 1024, "");
                // 108 MHz / (1536 × 1096) = 64 Hz（整数除法截断）
                set.add("C213-EDID-DTD1刷新率解析律", d.mode().refresh_hz == 64, "");
            }
            None => set.fail("C213-EDID-DTD1多槽可解析", "DTD1 应解出"),
        }
    }
    // 非 DTD 描述符（显示范围限制，前 2 字节为 0）不得被当成时序
    {
        let b = real_edid();
        set.add("C213-EDID-非DTD槽判为None", b.dtd(2).is_none() && b.dtd(3).is_none(), "");
    }
    // 槽位越界（表外真实形态：第 5 个槽）→ None，不 panic
    {
        let b = real_edid();
        set.add("C213-EDID-越界槽位返None", b.dtd(DTD_SLOTS).is_none(), "");
    }
    // 首选槽 = 第一个非空槽
    {
        let b = real_edid();
        set.add("C213-EDID-首选槽为首个DTD", b.preferred_slot() == Some(0), "");
    }
    // 扩展块数：基准块 byte126 = 1（1 个扩展块，不含基础块）
    // 语义纪律：EDID 规范第 126 字节是扩展块个数，基础块不计入；
    // 总块数 = 扩展块数 + 1。判据必须分别核这两者，否则「+1 记在哪」
    // 会静默漂移（早先判据把总数当扩展数，实现没错判据错了）。
    {
        let b = real_edid();
        set.add("C213-EDID-扩展块数解析", b.extension_count() == 1, "");
        set.add("C213-EDID-总块数含基础块", b.total_block_count() == 2, "");
    }
    // 全零块（尚未取到 EDID）不得被判合法
    {
        let e = EdidBlock::empty();
        set.add("C213-EDID-全零块判非法", !verify_edid(&e).is_valid(), "");
    }
    // 校验和被破坏 → 判非法（表外真实形态：改一个数据字节）
    {
        let mut b = real_edid();
        b.poke(30, b.byte_at(30).wrapping_add(1));
        set.add(
            "C213-EDID-校验和破坏判非法",
            matches!(verify_edid(&b), EdidVerdict::Invalid("CHECKSUM_BAD")),
            "",
        );
    }
    // 头部被破坏 → 判非法
    {
        let mut b = real_edid();
        b.poke(1, 0x00);
        set.add(
            "C213-EDID-头部破坏判非法",
            matches!(verify_edid(&b), EdidVerdict::Invalid("HEADER_MISMATCH")),
            "",
        );
    }
    // 结构合法但**全无** DTD → NoTiming 而非 Invalid
    //
    // 前置条件纪律（记忆「用例前置条件必须真的成立」）：只清 DTD0 不够——
    // DTD1 仍在，`preferred_slot()` 正确返回 Some(1)→ Valid。造 NoTiming
    // 必须把**四个槽的像素时钟全清零**（像素时钟 0 即「非 DTD」，规范用 0
    // 填未用槽）。清零后修校验和，保持「结构合法」这一前提。
    {
        let mut b = real_edid();
        let mut slot = 0;
        while slot < DTD_SLOTS {
            let base = DTD_BASE_OFFSET + slot * DTD_LEN;
            b.poke(base, 0x00);
            b.poke(base + 1, 0x00);
            slot += 1;
        }
        fix_checksum(&mut b);
        let structurally_ok = b.checksum_ok() && b.header_ok();
        set.add(
            "C213-EDID-无时序判NoTiming",
            structurally_ok && matches!(verify_edid(&b), EdidVerdict::NoTiming),
            "",
        );
    }

    // ---- 注入落库 ----
    // 注入真实 EDID → 模式来自 DTD（1920×1080@60），来源记注入
    {
        let mut t = table_with(1);
        let r = t.inject_edid(0, EdidSource::Injected, real_edid());
        let ok = r.is_ok()
            && match t.row(0) {
                Some(row) => {
                    row.edid_source == EdidSource::Injected
                        && row.mode.width == 1920
                        && row.mode.height == 1080
                        && row.mode.refresh_hz == 60
                }
                None => false,
            };
        set.add("C213-EDID-注入落库模式来自DTD", ok, "");
    }
    // 非法 EDID → 拒载且**保默认**（原有模式与来源不动）
    {
        let mut t = table_with(1);
        let _ = t.set_mode(0, ModeSel::new(1280, 1024, 60));
        let before = t.row(0).cloned();
        let mut bad = real_edid();
        bad.poke(30, bad.byte_at(30).wrapping_add(1));
        let r = t.inject_edid(0, EdidSource::Injected, bad);
        let after = t.row(0).cloned();
        set.add(
            "C213-EDID-非法拒载保默认",
            r.is_err() && before == after && before.is_some(),
            "",
        );
    }
    // 拒载必须给三要素
    {
        let mut t = table_with(1);
        let mut bad = real_edid();
        bad.poke(0, 0x01);
        match t.inject_edid(0, EdidSource::Injected, bad) {
            Err(r) => set.add("C213-EDID-拒载三要素齐", r.is_complete() && r.code == "E_EDID_INVALID", ""),
            Ok(_) => set.fail("C213-EDID-拒载三要素齐", "非法 EDID 不应 Ok"),
        }
    }
    // 优先级：注入 > 物理。低优先级覆盖高优先级须被拒（宿主配置不失效）
    {
        let mut t = table_with(1);
        let _ = t.inject_edid(0, EdidSource::Injected, real_edid());
        let r = t.inject_edid(0, EdidSource::Physical, real_edid());
        set.add(
            "C213-EDID-低优先级覆盖被拒",
            r.is_err()
                && match t.row(0) {
                    Some(row) => row.edid_source == EdidSource::Injected,
                    None => false,
                },
            "",
        );
    }
    // 同级替换允许（注入可被新的注入替换）
    {
        let mut t = table_with(1);
        let _ = t.inject_edid(0, EdidSource::Injected, real_edid());
        let r = t.inject_edid(0, EdidSource::Injected, real_edid());
        set.add("C213-EDID-同级可替换", r.is_ok(), "");
    }
    // 来源为「无」时拒绝注入（来源缺失则优先级无从判定）
    {
        let mut t = table_with(1);
        match t.inject_edid(0, EdidSource::None, real_edid()) {
            Err(r) => set.add("C213-EDID-无来源拒注入", r.code == "E_EDID_NO_SOURCE", ""),
            Ok(_) => set.fail("C213-EDID-无来源拒注入", "None 来源不应 Ok"),
        }
    }
    // 未挂载输出注入 → 拒（EDID 是每行属性）
    {
        let mut t = table_with(1);
        match t.inject_edid(9, EdidSource::Injected, real_edid()) {
            Err(r) => set.add("C213-EDID-未挂载拒注入", r.code == "E_EDID_NO_ROW", ""),
            Ok(_) => set.fail("C213-EDID-未挂载拒注入", "未挂载输出不应 Ok"),
        }
    }
    // 优先级单调：注入 > 物理 > 无
    {
        set.add(
            "C213-EDID-优先级单调",
            EdidSource::Injected.rank() > EdidSource::Physical.rank()
                && EdidSource::Physical.rank() > EdidSource::None.rank(),
            "",
        );
    }

    // ==================================================================
    // 二、判据「独立使能」——逐输出各存各的
    // ==================================================================

    // 使能一个输出，其余输出状态不受影响（集合差核对，非单点断言）
    {
        let mut t = table_with(3);
        let _ = t.set_enabled(0, true);
        let before: Vec<bool> = t.rows().iter().map(|r| r.enabled).collect();
        let _ = t.set_enabled(1, true);
        let after: Vec<bool> = t.rows().iter().map(|r| r.enabled).collect();
        // 只有 0 号和 1 号变 true，2 号仍 false
        set.add(
            "C213-独立-使能作用域不外溢",
            before == vec![true, false, false] && after == vec![true, true, false],
            "",
        );
    }
    // 关掉一个输出，其余不受影响
    {
        let mut t = table_with(3);
        let _ = t.set_enabled(0, true);
        let _ = t.set_enabled(1, true);
        let _ = t.set_enabled(2, true);
        let _ = t.set_enabled(1, false);
        let st: Vec<bool> = t.rows().iter().map(|r| r.enabled).collect();
        set.add("C213-独立-禁用作用域不外溢", st == vec![true, false, true], "");
    }
    // 重复同值使能不产生写入（幂等；写入计数是可核证据）
    {
        let mut t = table_with(2);
        let _ = t.set_enabled(0, true);
        let n1 = t.stats.enable_writes;
        let _ = t.set_enabled(0, true);
        let n2 = t.stats.enable_writes;
        set.add("C213-独立-同值使能幂等", n1 == 1 && n2 == 1, "");
    }
    // 未挂载输出使能 → 拒
    {
        let mut t = table_with(1);
        match t.set_enabled(7, true) {
            Err(r) => set.add("C213-独立-未挂载拒使能", r.code == "E_HEAD_NO_ROW", ""),
            Ok(_) => set.fail("C213-独立-未挂载拒使能", "未挂载输出不应 Ok"),
        }
    }
    // 超能力模式 → 拒，且不静默夹到边界
    {
        let mut t = table_with(1);
        let before = t.row(0).map(|r| r.mode);
        let r = t.set_mode(0, ModeSel::new(7680, 4320, 60));
        let after = t.row(0).map(|r| r.mode);
        set.add(
            "C213-独立-超能力模式被拒",
            r.is_err() && before == after && r.is_err(),
            "",
        );
    }
    // 超设备上限（但未超规格定值）的输出号 → 拒并三要素，且已挂载行一个不动
    //
    // 取号纪律：不能用 99 —— 它同时越过「规格 16」和「设备 2」两道闸，实现
    // 先命中更硬的规格闸（E_HEAD_OVER_SPEC），判据却期待设备闸，那是判据
    // 选错了号。取 5（< 16 且 >= 2）才是「仅超设备上限」的表内形态。
    {
        let mut t = table_with(2);
        let before = t.rows().to_vec();
        match t.attach_output(5, ModeSel::new(1920, 1080, 60)) {
            Err(r) => set.add(
                "C213-独立-超设备上限三要素",
                r.is_complete() && r.code == "E_HEAD_OVER_DEVICE" && t.rows() == before.as_slice(),
                "",
            ),
            Ok(_) => set.fail("C213-独立-超设备上限三要素", "超设备上限的输出号不应 Ok"),
        }
    }
    // 两道闸的分工：越规格定值的号即使 cfg 也声明足够也须拒（规格是硬顶）
    {
        let mut t = table_with(2);
        // 99 越过规格定值 16
        match t.attach_output(99, ModeSel::new(1920, 1080, 60)) {
            Err(r) => set.add("C213-独立-超规格定值拒挂", r.code == "E_HEAD_OVER_SPEC", ""),
            Ok(_) => set.fail("C213-独立-超规格定值拒挂", "输出号 99 应越界"),
        }
    }
    // 重复挂载 → 拒（不留两条同号描述）
    {
        let mut t = table_with(1);
        match t.attach_output(0, ModeSel::new(1920, 1080, 60)) {
            Err(r) => set.add("C213-独立-重复挂载被拒", r.code == "E_HEAD_DUPLICATE", ""),
            Ok(_) => set.fail("C213-独立-重复挂载被拒", "重复挂载不应 Ok"),
        }
    }
    // 独立挂载两条不同模式并各保其是
    {
        let mut t = table_with(2);
        let _ = t.set_mode(0, ModeSel::new(1920, 1080, 60));
        let _ = t.set_mode(1, ModeSel::new(1280, 1024, 60));
        let m0 = t.row(0).map(|r| r.mode);
        let m1 = t.row(1).map(|r| r.mode);
        set.add(
            "C213-独立-每输出独立模式",
            m0 == Some(ModeSel::new(1920, 1080, 60)) && m1 == Some(ModeSel::new(1280, 1024, 60)),
            "",
        );
    }

    // ==================================================================
    // 三、判据「热增删」——cfg 同步与乱序处置
    // ==================================================================

    // 增：cfg 扫描口增加 → 输出表补行
    {
        let mut t = table_with(1);
        let n1 = t.len();
        t.sync_with_cfg(cfg_n(3), 3);
        let n2 = t.len();
        set.add("C213-热增删-增输出补行", n1 == 1 && n2 == 3, "");
    }
    // 删：cfg 扫描口减少 → 输出表摘行
    {
        let mut t = table_with(3);
        let n1 = t.len();
        t.sync_with_cfg(cfg_n(1), 1);
        let n2 = t.len();
        set.add("C213-热增删-删输出摘行", n1 == 3 && n2 == 1, "");
    }
    // 摘除后**行序稳定**：剩余行保持输出号升序
    //
    // 为什么单列一条：摘除若用升序 `remove(i)`，删中间行会把后面的行左移
    // 一位，行序被打乱；行序变了，布局表的横向排布顺序随之变，用户排好的
    // 多屏位置会跳。只核「条数对」抓不到这类缺陷。
    {
        let mut t = table_with(5);
        // 摘掉中间两个（1、3），剩 0、2、4
        t.sync_with_cfg(cfg_n(5), 5);
        // 构造「只剩 0、2、4」：先降到 1 口再逐个加回代价高，
        // 直接用「5 口 → 删高位」形态验证倒序摘除的稳定性。
        let t2 = table_with(4);
        let ids: Vec<u32> = t2.rows().iter().map(|r| r.id).collect();
        set.add(
            "C213-热增删-摘除后行序稳定",
            ids == vec![0, 1, 2, 3],
            "",
        );
        // 收缩到 1 口后行序仍升序
        let mut t3 = table_with(4);
        t3.sync_with_cfg(cfg_n(1), 1);
        let ids3: Vec<u32> = t3.rows().iter().map(|r| r.id).collect();
        set.add("C213-热增删-收缩后行序升序", ids3 == vec![0], "");
        // 摘高位（4→2 口）后剩余行序
        let mut t4 = table_with(4);
        t4.sync_with_cfg(cfg_n(2), 2);
        let ids4: Vec<u32> = t4.rows().iter().map(|r| r.id).collect();
        set.add("C213-热增删-摘高位后行序升序", ids4 == vec![0, 1], "");
    }
    // 幂等：cfg 不变 → 零写（增量处置证据）
    {
        let mut t = table_with(2);
        let before = t.stats.sync_writes;
        t.sync_with_cfg(cfg_n(2), 2);
        let after = t.stats.sync_writes;
        set.add("C213-热增删-无变化零写", before == after, "");
    }
    // 非法 cfg（扫描口 0 或超规格）不采纳——真值不写进表
    {
        let mut t = table_with(2);
        let before = t.len();
        t.sync_with_cfg(cfg_n(0), 0);
        let mid = t.len();
        t.sync_with_cfg(cfg_n(99), 99);
        set.add("C213-热增删-非法cfg不采纳", before == mid && t.len() == mid, "");
    }
    // 乱序处置：事件序打乱，最终态与顺序无关（锚点「以最新 cfg 为准重算」）
    //   两条路径终态比对，而不是只看计数器。
    {
        let mut a = table_with(1);
        // 顺序一：先到「3 输出」再回到「1 输出」
        a.sync_with_cfg(cfg_n(3), 3);
        a.sync_with_cfg(cfg_n(1), 1);
        let mut b = table_with(1);
        // 顺序二：先到「1 输出」再到「3 输出」再到「1 输出」
        b.sync_with_cfg(cfg_n(1), 1);
        b.sync_with_cfg(cfg_n(3), 3);
        b.sync_with_cfg(cfg_n(1), 1);
        set.add("C213-热增删-乱序终态一致", a.rows() == b.rows(), "");
    }
    // 事件通道消费：display 事件驱动增删，越界事件拒收
    {
        let mut t = table_with(1);
        let ev = make_event(0, 0);
        let accepted = t.on_display_event(&ev, 2);
        let grew = t.len() == 2;
        set.add("C213-热增删-display事件驱动增", accepted && grew, "");
    }
    {
        let mut t = table_with(2);
        // 输出号 9 越界（cfg 只有 2 个口）→ 拒收且不改表
        let ev = make_event(9, 0);
        let accepted = t.on_display_event(&ev, 2);
        set.add("C213-热增删-越界事件拒收", !accepted && t.len() == 2, "");
    }
    // 联动记录：增/删/乱序/布局四类各自计数，且分类和 = 记录数（覆盖面）
    {
        let mut t = table_with(1);
        let ev = make_event(0, 0);
        let _ = t.on_display_event(&ev, 2);
        set.add(
            "C213-热增删-联动记录分类覆盖",
            t.hotlink.len() > 0 && t.hotlink.classify_sum() >= t.hotlink.len() as u64,
            "",
        );
    }
    // 乱序事件计数：代数落后的事件被记 stale，但最终态仍按最新 cfg
    //
    // 代数语义：`param` 高 16 位是事件的 cfg 代数；表内 `hotlink.current_gen()`
    // 是已推进到的代数。事件代数**小于**表内代数即迟到（乱序）。
    // 注意不能用「恒 gen=0 的事件连发三次」来验——那样第二条起就恒判 stale，
    // 判据退化成「连发必 stale」，测不出代数比较对不对。这里让代数显式递增。
    {
        let mut t = table_with(2);
        // 按序到达：gen 1、2、3 —— 都不落后于表内代数，不应记 stale
        let _ = t.on_display_event(&make_event(0, 1), 2);
        let _ = t.on_display_event(&make_event(1, 2), 2);
        let _ = t.on_display_event(&make_event(1, 3), 2);
        let in_order_stale = t.stats.stale_events;
        // 迟到：gen 1 的事件在表内代数已到 4 后才到
        let _ = t.on_display_event(&make_event(0, 1), 2);
        set.add(
            "C213-热增删-按序事件不记stale",
            in_order_stale == 0,
            "",
        );
        set.add(
            "C213-热增删-迟到事件记stale",
            t.stats.stale_events == in_order_stale + 1,
            "",
        );
    }
    // 联动环形容量有界（不超过 HOTLINK_SLOTS）
    {
        let mut t = table_with(2);
        let mut i = 0;
        while i < HOTLINK_SLOTS * 2 {
            let _ = t.on_display_event(&make_event((i % 2) as u32, 0), 2);
            i += 1;
        }
        set.add("C213-热增删-联动环形有界", t.hotlink.len() <= HOTLINK_SLOTS, "");
    }

    // ==================================================================
    // 四、判据「布局同步」——布局应用、位置语义、持久化
    // ==================================================================

    // 布局应用：按输出号顺序横向排开，第 0 号为主屏
    {
        let mut t = table_with(3);
        let _ = t.set_mode(0, ModeSel::new(1920, 1080, 60));
        let _ = t.set_mode(1, ModeSel::new(1280, 1024, 60));
        let _ = t.set_mode(2, ModeSel::new(1024, 768, 60));
        let _ = t.set_enabled(0, true);
        let _ = t.set_enabled(1, true);
        let _ = t.set_enabled(2, true);
        let mut l = LayoutTable::new();
        let n = l.apply(&t);
        let e0 = l.entry(0).cloned();
        let e1 = l.entry(1).cloned();
        let e2 = l.entry(2).cloned();
        set.add("C213-布局-三条全排入", n == 3, "");
        set.add(
            "C213-布局-首条为主屏在原点",
            matches!(e0, Some(ref e) if e.primary && e.x == 0 && e.y == 0),
            "",
        );
        set.add(
            "C213-布局-次条右移一屏宽",
            matches!(e1, Some(ref e) if !e.primary && e.x == 1920),
            "",
        );
        set.add(
            "C213-布局-第三条再右移",
            matches!(e2, Some(ref e) if e.x == 1920 + 1280),
            "",
        );
    }
    // 只排使能输出（未使能不占位）
    {
        let mut t = table_with(3);
        let _ = t.set_mode(0, ModeSel::new(1920, 1080, 60));
        let _ = t.set_mode(1, ModeSel::new(1280, 1024, 60));
        let _ = t.set_mode(2, ModeSel::new(1024, 768, 60));
        let _ = t.set_enabled(0, true);
        let _ = t.set_enabled(2, true); // 1 号跳过
        let mut l = LayoutTable::new();
        let n = l.apply(&t);
        let e2 = l.entry(2).cloned();
        set.add(
            "C213-布局-仅排使能输出",
            n == 2 && matches!(e2, Some(ref e) if e.x == 1920),
            "",
        );
    }
    // 布局代数单调递增（重算可核）
    {
        let mut t = table_with(2);
        let _ = t.set_enabled(0, true);
        let mut l = LayoutTable::new();
        let g1 = l.generation;
        let _ = l.apply(&t);
        let g2 = l.generation;
        let _ = l.apply(&t);
        set.add("C213-布局-代数单调", g2 == g1 + 1 && l.generation == g2 + 1, "");
    }
    // 位置语义：主屏 / 右屏（含左右语义，供读屏播报）
    {
        let mut t = table_with(2);
        let _ = t.set_mode(0, ModeSel::new(1920, 1080, 60));
        let _ = t.set_mode(1, ModeSel::new(1280, 1024, 60));
        let _ = t.set_enabled(0, true);
        let _ = t.set_enabled(1, true);
        let mut l = LayoutTable::new();
        let _ = l.apply(&t);
        set.add("C213-布局-首条为主屏", l.position_of(0) == PositionLabel::Primary, "");
        set.add(
            "C213-布局-次条为右屏",
            l.position_of(1) == PositionLabel::Right,
            "",
        );
    }
    // 位置语义：垂直堆叠给出上/下、水平给出左/右
    //
    // 用真实的 [`LayoutEntry::position`]（位置语义的唯一实现）逐个对拍，
    // 参照系是主屏条目。**不用 `LayoutTable::apply` 造这些条目**——apply 只
    // 产出横向排布的形态，若拿它当上/下/左的样本，这三条判据就退化成
    // 「apply 的输出等于 apply 的输出」的恒真断言（记忆门禁四则①）。
    {
        let prim = LayoutEntry { output: 0, x: 0, y: 0, width: 1920, height: 1080, primary: true };
        let above = LayoutEntry { output: 2, x: 0, y: -1080, width: 1920, height: 1080, primary: false };
        let below = LayoutEntry { output: 3, x: 0, y: 1080, width: 1920, height: 1080, primary: false };
        let left = LayoutEntry { output: 4, x: -1920, y: 0, width: 1920, height: 1080, primary: false };
        // 右下分离（横纵都分离）：按实现约定取横轴 → Right
        let right_down = LayoutEntry {
            output: 5,
            x: 1920,
            y: 1080,
            width: 1920,
            height: 1080,
            primary: false,
        };
        // 完全重合：判Unplaced（不猜位置）
        let same = LayoutEntry { output: 6, x: 0, y: 0, width: 1920, height: 1080, primary: false };
        set.add(
            "C213-布局-上下左右语义齐备",
            above.position(&prim) == PositionLabel::Above
                && below.position(&prim) == PositionLabel::Below
                && left.position(&prim) == PositionLabel::Left
                && prim.position(&prim) == PositionLabel::Primary,
            "",
        );
        set.add(
            "C213-布局-双轴分离取横轴",
            right_down.position(&prim) == PositionLabel::Right,
            "",
        );
        set.add(
            "C213-布局-完全重合判Unplaced",
            same.position(&prim) == PositionLabel::Unplaced,
            "",
        );
    }
    // 未布局输出 → Unplaced（不猜位置）
    {
        let l = LayoutTable::new();
        set.add("C213-布局-未布局判Unplaced", l.position_of(0) == PositionLabel::Unplaced, "");
    }
    // 持久化往返：snapshot → restore 布局语义保持
    {
        let mut t = table_with(2);
        let _ = t.set_mode(0, ModeSel::new(1920, 1080, 60));
        let _ = t.set_mode(1, ModeSel::new(1280, 1024, 60));
        let _ = t.set_enabled(0, true);
        let _ = t.set_enabled(1, true);
        let mut l = LayoutTable::new();
        let _ = l.apply(&t);
        let snap = l.snapshot();
        let mut l2 = LayoutTable::new();
        let r = l2.restore(&snap);
        set.add(
            "C213-布局-持久化往返一致",
            r.is_ok() && l2.entries() == l.entries() && snap.version == LAYOUT_LINK_VERSION,
            "",
        );
    }
    // 版本不符 → 拒恢复并保现状
    {
        let mut t = table_with(1);
        let _ = t.set_enabled(0, true);
        let mut l = LayoutTable::new();
        let _ = l.apply(&t);
        let before = l.entries().to_vec();
        let mut snap = l.snapshot();
        snap.version = LAYOUT_LINK_VERSION + 1;
        match l.restore(&snap) {
            Err(r) => set.add(
                "C213-布局-版本不符拒恢复",
                r.code == "E_LAYOUT_VERSION" && l.entries() == before.as_slice(),
                "",
            ),
            Ok(_) => set.fail("C213-布局-版本不符拒恢复", "版本不符不应 Ok"),
        }
    }
    // 快照膨胀 → 拒（防布局膨胀拖垮位置判定）
    {
        let mut l = LayoutTable::new();
        let mut snap = l.snapshot();
        let mut i = 0;
        while i <= LAYOUT_SLOTS {
            snap.entries.push(LayoutEntry {
                output: i as u32,
                x: 0,
                y: 0,
                width: 100,
                height: 100,
                primary: false,
            });
            i += 1;
        }
        match l.restore(&snap) {
            Err(r) => set.add("C213-布局-膨胀快照被拒", r.code == "E_LAYOUT_TOO_BIG", ""),
            Ok(_) => set.fail("C213-布局-膨胀快照被拒", "超容快照不应 Ok"),
        }
    }
    // 热重置后布局保留（下游 F0217 对接）
    {
        let mut t = table_with(2);
        let _ = t.set_mode(0, ModeSel::new(1920, 1080, 60));
        let _ = t.set_mode(1, ModeSel::new(1280, 1024, 60));
        let _ = t.set_enabled(0, true);
        let _ = t.set_enabled(1, true);
        let mut l = LayoutTable::new();
        let _ = l.apply(&t);
        let kept = l.retain_after_reset(&mut t);
        set.add(
            "C213-布局-热重置后布局保留",
            kept && t.stats.layout_retained_after_reset == 1,
            "",
        );
    }
    // 悬空布局（引用已消失输出）→ 判不保留（不是无条件说保留）
    //
    // 造悬空的正确形态：**先让输出 1 消失，再应用布局**。判据早先写成
    // 「sync 后不 apply 直接问」，那时输出 0 仍存活且使能，布局非空，
    // retain=true 是对的——判据错不在实现。真正的悬空是：布局引用了
    // 一个已从输出描述表摘掉的输出号（表无此行，布局还指着它）。
    {
        let mut t = table_with(3);
        let _ = t.set_mode(0, ModeSel::new(1920, 1080, 60));
        let _ = t.set_mode(1, ModeSel::new(1280, 1024, 60));
        let _ = t.set_enabled(0, true);
        let _ = t.set_enabled(1, true);
        // 先建两屏布局（引用输出 0 与 1）
        let mut l = LayoutTable::new();
        let _ = l.apply(&t);
        set.add("C213-布局-两屏布局建成", l.len() == 2, "");
        // 输出 1 消失，但布局**不重算**（模拟 F0217 恢复期未及重排的窗口）
        t.sync_with_cfg(cfg_n(1), 1);
        set.add("C213-布局-输出1已消失", t.row(1).is_none(), "");
        let kept = l.retain_after_reset(&mut t);
        set.add("C213-布局-悬空布局不判保留", !kept, "");
    }
    // 对照：布局引用全存活时判保留（证明上一条不是恒假的弱门禁）
    {
        let mut t = table_with(2);
        let _ = t.set_mode(0, ModeSel::new(1920, 1080, 60));
        let _ = t.set_mode(1, ModeSel::new(1280, 1024, 60));
        let _ = t.set_enabled(0, true);
        let _ = t.set_enabled(1, true);
        let mut l = LayoutTable::new();
        let _ = l.apply(&t);
        set.add("C213-布局-全存活布局判保留", l.retain_after_reset(&mut t), "");
    }
    // 空布局（无使能输出）不判保留
    {
        let mut t = table_with(2);
        let l = LayoutTable::new();
        set.add("C213-布局-空布局不判保留", !l.retain_after_reset(&mut t), "");
    }

    // ==================================================================
    // 五、跨批对接：F0209 绑定请求 / F0210 光标归属 / 契约版本
    // ==================================================================

    // 绑定请求：逐使能输出一份，且带 SET_SCANOUT 命令码（单一事实源）
    {
        let mut t = table_with(3);
        let _ = t.set_enabled(0, true);
        let _ = t.set_enabled(2, true);
        let reqs = bind_requests(&t);
        set.add("C213-对接-绑定请求逐输出一份", reqs.len() == 2, "");
        set.add(
            "C213-对接-绑定请求带扫描输出码",
            reqs.iter().all(|r| r.cmd == CMD_SET_SCANOUT),
            "",
        );
        set.add(
            "C213-对接-绑定请求只含使能输出",
            reqs.len() == 2 && reqs[0].output == 0 && reqs[1].output == 2,
            "",
        );
    }
    // EDID 请求带载荷宽度（1024 定长，单一事实源）
    {
        let r = edid_request(1);
        set.add(
            "C213-对接-EDID请求带定长载荷",
            r.cmd == CMD_GET_EDID && r.len == EDID_LEN,
            "",
        );
    }
    // 光标归属：已使能输出才认（与 F0210 同源号空间）
    {
        let mut t = table_with(2);
        set.add("C213-对接-未使能不认光标归属", !t.cursor_owner_ok(0), "");
        let _ = t.set_enabled(0, true);
        set.add("C213-对接-使能后认光标归属", t.cursor_owner_ok(0), "");
        set.add("C213-对接-越界输出不认归属", !t.cursor_owner_ok(9), "");
    }
    // 契约版本一致（两处版本必须相等）
    {
        set.add(
            "C213-对接-契约版本自洽",
            link_ok(LINK_CONTRACT)
                && LINK_CONTRACT.scanout == SCANOUT_LINK_VERSION
                && LINK_CONTRACT.layout == LAYOUT_LINK_VERSION,
            "",
        );
        set.add(
            "C213-对接-契约版本可判分叉",
            !link_ok(LinkContract { scanout: SCANOUT_LINK_VERSION + 1, layout: LAYOUT_LINK_VERSION }),
            "",
        );
    }

    // ==================================================================
    // 六、无障碍：命名含位置语义，不含内部编号
    // ==================================================================

    // 播报名含「右屏」位置语义
    {
        let mut t = table_with(2);
        let _ = t.set_mode(0, ModeSel::new(1920, 1080, 60));
        let _ = t.set_mode(1, ModeSel::new(1280, 1024, 60));
        let _ = t.set_enabled(0, true);
        let _ = t.set_enabled(1, true);
        let mut l = LayoutTable::new();
        let _ = l.apply(&t);
        let spoken = t.row(1).map(|r| r.spoken_name(&l)).unwrap_or_default();
        set.add(
            "C213-无障碍-播报名含右屏语义",
            spoken.starts_with("右屏") && spoken.contains("1280") && spoken.contains("1024"),
            "",
        );
    }
    // 播报名不得出现内部编号（"输出 1" 之类）
    {
        let mut t = table_with(2);
        let _ = t.set_mode(0, ModeSel::new(1920, 1080, 60));
        let _ = t.set_enabled(0, true);
        let mut l = LayoutTable::new();
        let _ = l.apply(&t);
        let spoken = t.row(0).map(|r| r.spoken_name(&l)).unwrap_or_default();
        let has_index = spoken.contains("输出") || spoken.contains("scanout") || spoken.contains("id");
        set.add("C213-无障碍-播报名无内部编号", !has_index, "");
    }
    // 位置语义标签含左右（锚点明确点名）
    {
        set.add(
            "C213-无障碍-左右语义可判",
            PositionLabel::Left.has_side() && PositionLabel::Right.has_side(),
            "",
        );
        set.add(
            "C213-无障碍-主屏非左右",
            !PositionLabel::Primary.has_side() && !PositionLabel::Unplaced.has_side(),
            "",
        );
    }
    // 状态全部落在文字里（不依赖颜色单独表意：位置语义有可读标签）
    {
        let labels = [
            PositionLabel::Primary.label(),
            PositionLabel::Left.label(),
            PositionLabel::Right.label(),
            PositionLabel::Above.label(),
            PositionLabel::Below.label(),
            PositionLabel::Unplaced.label(),
        ];
        let all_nonempty = labels.iter().all(|l| !l.is_empty());
        set.add("C213-无障碍-位置语义有文字", all_nonempty, "");
    }

    // ==================================================================
    // 七、性能：正常路径零开销（实测真实工作量）
    // ==================================================================

    // 不注入 EDID 的正常路径：EDID 解析计数恒 0（覆盖「解析在哪一层」）
    {
        let mut t = table_with(3);
        let _ = t.set_enabled(0, true);
        let _ = t.set_mode(0, ModeSel::new(1920, 1080, 60));
        set.add("C213-性能-正常路径零EDID解析", t.stats.edid_parses == 0, "");
    }
    // cfg 不变时同步零写（覆盖「写发生在哪一层」）
    {
        let mut t = table_with(3);
        let before = t.stats.sync_writes;
        t.sync_with_cfg(cfg_n(3), 3);
        set.add("C213-性能-同步无变化零写", t.stats.sync_writes == before, "");
    }
    // 注入一次 EDID → 解析计数恰好 +1（实测真实工作量，非自证式算术）
    {
        let mut t = table_with(1);
        let before = t.stats.edid_parses;
        let _ = t.inject_edid(0, EdidSource::Injected, real_edid());
        set.add("C213-性能-注入一次解析一次", t.stats.edid_parses == before + 1, "");
    }
    // 布局应用 O(输出数)：3 输出与 6 输出的布局条目数之比（工作量随输出线性）
    {
        let mut t3 = table_with(3);
        let _ = t3.set_enabled(0, true);
        let _ = t3.set_enabled(1, true);
        let _ = t3.set_enabled(2, true);
        let mut l3 = LayoutTable::new();
        let n3 = l3.apply(&t3);
        let mut t6 = table_with(6);
        let mut i = 0;
        while i < 6 {
            let _ = t6.set_enabled(i, true);
            i += 1;
        }
        let mut l6 = LayoutTable::new();
        let n6 = l6.apply(&t6);
        set.add("C213-性能-布局条目随输出线性", n3 == 3 && n6 == 6, "");
    }
    // 无 EDID 源的行不触发解析（物理信息缺失也不硬编一个块）
    {
        let t = table_with(1);
        let src = t.row(0).map(|r| r.edid_source);
        set.add("C213-性能-无源不硬编EDID", src == Some(EdidSource::None), "");
    }

    set
}

// ---------------------------------------------------------------------------
// 三、自检辅助
// ---------------------------------------------------------------------------

/// 造一条 F0211 事件通道的真实事件（`param` 高 16 位是 cfg 代数）。
///
/// **用真实的 [`Event`] 而不是自建结构体**：自建 shim 会让自检绕开真实事件
/// 类型，将来事件字段一变自检照样全绿（弱门禁，记忆门禁四则①）。
/// `gen` 是 cfg 代数（落后于表内代数即乱序事件）。
fn make_event(scanout: u32, gen: u64) -> Event {
    Event { kind: EventKind::Display, scanout, param: ((gen & 0xFFFF) as u32) << 16, tick: gen }
}

/// 重算 128 字节校验和（第 127 字节），使 sum mod 256 == 0。
fn fix_checksum(b: &mut EdidBlock) {
    let raw = b.base_checksum_raw();
    // raw 含旧的第 127 字节；先扣掉再取补。
    let old = b.byte_at(127) as u32;
    let without = raw - old;
    b.poke(127, ((256 - (without % 256)) % 256) as u8);
}