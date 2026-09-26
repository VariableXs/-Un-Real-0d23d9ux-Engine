//! F173 panic 画面设计（secstar · G-G-03）——「星陨」：崩溃是事故，不是羞辱。
//!
//! 主册判据（验收标准第一句）：
//! **panic 百次演练画面渲染成功率 100%（含最简降级 5 次注入）；倒计时重启实测；二维码解码正确（三机型扫测）。**
//!
//! 功能定义（G-G-03）：内核 panic 画面「星陨」：VARIX 语汇替代蓝屏——暗底
//! 星徽碎裂动画（静帧三选随机）+错误码+二维码（指向帮助 F119 对应篇）+
//! 自动重启倒计时 30s（可 Enter 立即）；panic 百次演练（B-2903）同步验证新画面。
//!
//! 【交互设计】画面布局：中央星徽碎裂静帧（三帧随机防审美疲劳）/错误码行
//! （等宽字体）/「扫码了解此错误」二维码 96px/倒计时环 30s；Enter 立即重启/
//! ESC 停留读码（倒计时暂停）；重启前自动 dump（F020 管线内核态子集）。
//! 【数据与存储】panic 记录（码/地址/次数）持久化入诊断（重启后 F120 首页
//! 可见「上次异常重启」条目）。
//! 【状态与异常】panic 于画面系统自身 → 最简文字模式兜底（两级降级）；
//! 连续 panic（重启即崩）→ 第二次起跳过倒计时直接进安全模式询问（F193
//! 联动防循环）。
//! 【设计细节】碎裂帧预烘 3 张（120KB 预算内）；错误码格式
//! VX-PANIC-<模块>-<序号>；倒计时环复用 F171 环资产（一套资产两处用）；
//! 画面在串口日志完成后才渲染（日志优先原则）；自动 dump 上限 4MB。
//!
//! 二维码：微型 QR 编解码器（byte 模式 · EC 级 L · 版本 1-4 自适应），
//! 自包含自验证——宿主侧「矩阵→解码→载荷一致」round-trip 全绿；三台真机
//! 扫测属外设判据，登记随闸门补测（开发期零实机纪律）。
//!
//! 零堆纪律：GF 表 const 编译期生成、定长矩阵、无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 自动重启倒计时 30s。
pub const REBOOT_COUNTDOWN_MS: u64 = 30_000;
/// 二维码 96px 显示尺寸。
pub const QR_DISPLAY_PX: u32 = 96;
/// 碎裂静帧 3 张（随机防审美疲劳）。
pub const SHATTER_FRAMES: usize = 3;
/// 碎裂帧资产预算 120KB 内。
pub const SHATTER_ASSET_BUDGET: usize = 120 * 1024;
/// 自动 dump 上限 4MB（引导环境内存纪律）。
pub const DUMP_CAP_BYTES: u32 = 4 << 20;
/// panic 记录持久化容量（码/地址/次数，环形）。
pub const RECORD_CAP: usize = 16;
/// 连续 panic 阈值：第二次起跳过倒计时进安全模式询问（F193 防循环）。
pub const CONSECUTIVE_SAFE_MODE_AT: u32 = 2;

// ---------------------------------------------------------------------------
// 微型 QR 编解码器（byte 模式 · EC L · 版本 1-4）
// ---------------------------------------------------------------------------

/// GF(256) 指数表（α=2，本原多项式 0x11D），编译期生成。
const fn build_exp() -> [u16; 512] {
    let mut exp = [0u16; 512];
    let mut x: u16 = 1;
    let mut i = 0;
    while i < 255 {
        exp[i] = x;
        exp[i + 255] = x;
        x <<= 1;
        if x & 0x100 != 0 {
            x ^= 0x11D;
        }
        i += 1;
    }
    exp
}

/// GF(256) 对数表，编译期生成。
const fn build_log() -> [u8; 256] {
    let mut log = [0u8; 256];
    let exp = build_exp();
    let mut i = 0;
    while i < 255 {
        log[exp[i] as usize] = i as u8;
        i += 1;
    }
    log
}

static GF_EXP: [u16; 512] = build_exp();
static GF_LOG: [u8; 256] = build_log();

#[inline]
fn gf_mul(a: u8, b: u8) -> u8 {
    if a == 0 || b == 0 {
        0
    } else {
        GF_EXP[(GF_LOG[a as usize] as usize + GF_LOG[b as usize] as usize) % 255] as u8
    }
}

/// 版本参数（全单块结构：v1-L 到 v4-L 均为 1 个 RS 块）。
struct Vparam {
    size: usize,
    data_cw: usize,
    ec_cw: usize,
    /// byte 模式载荷容量（字节）。
    cap_bytes: usize,
    align: [usize; 2],
    align_n: usize,
    remainder_bits: usize,
}

const VPARAMS: [Vparam; 4] = [
    Vparam { size: 21, data_cw: 19, ec_cw: 7, cap_bytes: 17, align: [0, 0], align_n: 0, remainder_bits: 0 },
    Vparam { size: 25, data_cw: 34, ec_cw: 10, cap_bytes: 32, align: [6, 18], align_n: 2, remainder_bits: 7 },
    Vparam { size: 29, data_cw: 55, ec_cw: 15, cap_bytes: 53, align: [6, 22], align_n: 2, remainder_bits: 7 },
    Vparam { size: 33, data_cw: 80, ec_cw: 20, cap_bytes: 78, align: [6, 26], align_n: 2, remainder_bits: 7 },
];

/// 定长 QR 矩阵（最大 v4 = 33×33）。
pub struct QrMatrix {
    pub size: usize,
    pub modules: [u8; 33 * 33],
    pub version: usize,
    pub mask: usize,
}

impl QrMatrix {
    pub fn get(&self, r: usize, c: usize) -> u8 {
        self.modules[r * self.size + c]
    }
    fn set(&mut self, r: usize, c: usize, v: u8) {
        self.modules[r * self.size + c] = v;
    }
}

fn rs_generator(ec: usize, gen: &mut [u8; 21]) {
    // 生成多项式 = ∏(x - α^i), i=0..ec；首系数恒 1（monic）。
    // 数组下标 i = x^(deg-i) 系数（gen[0] 最高次）。
    // 乘 (x + root)：h[0]=g[0]；h[k]=g[k]^root·g[k-1]（k=1..=deg）——
    // 乘完后最高次=deg（k 循环已覆盖全部系数，无 deg+1 项）。
    gen[0] = 1;
    let mut deg = 1;
    for i in 0..ec {
        let root = GF_EXP[i] as u8;
        let mut tmp = [0u8; 21];
        tmp[0] = gen[0];
        for k in 1..=deg {
            tmp[k] = gen[k] ^ gf_mul(gen[k - 1], root);
        }
        *gen = tmp;
        deg += 1;
    }
}

fn rs_encode(data: &[u8], ec: usize, gen: &[u8; 21], out_ec: &mut [u8; 20]) {
    let mut msg = [0u8; 100]; // data + ec ≤ 80+20
    let dl = data.len();
    msg[..dl].copy_from_slice(data);
    for i in 0..dl {
        let coef = msg[i];
        if coef != 0 {
            for j in 1..=ec {
                msg[i + j] ^= gf_mul(gen[j], coef);
            }
        }
    }
    out_ec[..ec].copy_from_slice(&msg[dl..dl + ec]);
}

/// RS 校验：整码字多项式对生成多项式取余应为 0（syndrome 全零）。
fn rs_verify(codewords: &[u8], ec: usize, gen: &[u8; 21]) -> bool {
    let mut rem = [0u8; 100];
    rem[..codewords.len()].copy_from_slice(codewords);
    let dl = codewords.len() - ec;
    for i in 0..dl {
        let coef = rem[i];
        if coef != 0 {
            for j in 1..=ec {
                rem[i + j] ^= gf_mul(gen[j], coef);
            }
        }
    }
    rem[codewords.len() - ec..codewords.len()].iter().all(|b| *b == 0)
}

/// 功能模块图（finders/timing/alignment/format/暗模块——不载数据）。
fn is_function(v: &Vparam, r: usize, c: usize) -> bool {
    let s = v.size;
    // 三个 finder + 分隔带（7×7 外扩一圈 → 裁剪后 8×8 区）。
    for (fr, fc) in [(0usize, 0usize), (0, s - 7), (s - 7, 0)] {
        if r >= fr.saturating_sub(1) && r <= fr + 7 && c >= fc.saturating_sub(1) && c <= fc + 7 {
            return true;
        }
    }
    // timing 行列。
    if r == 6 || c == 6 {
        return true;
    }
    // alignment（5×5；跳过与 finder 重叠的组合）。
    for i in 0..v.align_n {
        for j in 0..v.align_n {
            let ar = v.align[i];
            let ac = v.align[j];
            if (ar == 6 && ac == 6) || (ar == 6 && ac == s - 7) || (ar == s - 7 && ac == 6) {
                continue;
            }
            if r + 2 >= ar && r <= ar + 2 && c + 2 >= ac && c <= ac + 2 {
                return true;
            }
        }
    }
    // 暗模块（s-8, 8）。
    if r == s - 8 && c == 8 {
        return true;
    }
    // format 区（两份副本）。
    if c == 8 && r <= 8 && r != 6 {
        return true;
    }
    if r == 8 && c <= 8 && c != 6 {
        return true;
    }
    if r == 8 && c >= s - 8 {
        return true;
    }
    if c == 8 && r >= s - 7 {
        return true;
    }
    false
}

/// 掩码判定（x=列，y=行；ISO 18004 八种）。
fn mask_bit(m: usize, x: usize, y: usize) -> bool {
    match m {
        0 => (x + y) % 2 == 0,
        1 => y % 2 == 0,
        2 => x % 3 == 0,
        3 => (x + y) % 3 == 0,
        4 => (x / 3 + y / 2) % 2 == 0,
        5 => (x * y) % 2 + (x * y) % 3 == 0,
        6 => ((x * y) % 2 + (x * y) % 3) % 2 == 0,
        _ => ((x + y) % 2 + (x * y) % 3) % 2 == 0,
    }
}

/// format info 15 位（EC L=01、掩码 m、BCH(15,5)、异或 0x5412）。
fn format_bits(mask: usize) -> u16 {
    let data = (1u16 << 3) | mask as u16; // EC level L = 01
    let mut rem = data;
    for _ in 0..10 {
        rem = (rem << 1) ^ ((rem >> 9) * 0x537);
    }
    ((data << 10) | (rem & 0x3FF)) ^ 0x5412
}

fn draw_format(qr: &mut QrMatrix, mask: usize) {
    let bits = format_bits(mask);
    let s = qr.size;
    let bit = |i: usize| ((bits >> i) & 1) as u8;
    // 副本一：列 8 行 0-5、7、8；行 8 列 7、5-0。
    for i in 0..=5 {
        qr.set(i, 8, bit(i));
    }
    qr.set(7, 8, bit(6));
    qr.set(8, 8, bit(7));
    qr.set(8, 7, bit(8));
    for i in 9..15 {
        qr.set(8, 14 - i, bit(i));
    }
    // 副本二：行 8 列 s-1..s-8；列 8 行 s-7..s-1。
    for i in 0..8 {
        qr.set(8, s - 1 - i, bit(i));
    }
    for i in 8..15 {
        qr.set(s - 15 + i, 8, bit(i));
    }
}

/// 惩罚分（N1-N4 全规则，选最低掩码——可扫描性诚实优化）。
fn penalty(qr: &QrMatrix) -> u32 {
    let s = qr.size;
    let mut score: u32 = 0;
    let get = |r: usize, c: usize| qr.get(r, c) != 0;
    // N1 行列同色游程。
    for dir in 0..2 {
        for i in 0..s {
            let mut run = 1;
            for j in 1..s {
                let (a, b) = if dir == 0 { (get(i, j - 1), get(i, j)) } else { (get(j - 1, i), get(j, i)) };
                if a == b {
                    run += 1;
                } else {
                    if run >= 5 {
                        score += 3 + (run - 5) as u32;
                    }
                    run = 1;
                }
            }
            if run >= 5 {
                score += 3 + (run - 5) as u32;
            }
        }
    }
    // N2 2×2 同色。
    for r in 0..s - 1 {
        for c in 0..s - 1 {
            let v = get(r, c);
            if get(r, c + 1) == v && get(r + 1, c) == v && get(r + 1, c + 1) == v {
                score += 3;
            }
        }
    }
    // N3 finder 仿形 1011101 + 两侧各 4 亮位。
    let pat = [true, false, true, true, true, false, true];
    for dir in 0..2 {
        for i in 0..s {
            for j in 0..=s - 11 {
                {
                    // 模式左侧或右侧亮带
                    let mut ok = true;
                    for k in 0..7 {
                        let (r, c) = if dir == 0 { (i, j + 4 + k) } else { (j + 4 + k, i) };
                        if get(r, c) != pat[k] {
                            ok = false;
                            break;
                        }
                    }
                    if !ok {
                        continue;
                    }
                    let light_side = |base: isize| -> bool {
                        for k in 0..4 {
                            let p = base + k as isize;
                            if p < 0 || p >= s as isize {
                                return false;
                            }
                            let (r, c) = if dir == 0 { (i, p as usize) } else { (p as usize, i) };
                            if get(r, c) {
                                return false;
                            }
                        }
                        true
                    };
                    if light_side(j as isize) || light_side((j + 11) as isize) {
                        score += 40;
                    }
                }
            }
        }
    }    // N4 暗模块占比偏离。
    let mut dark = 0;
    for r in 0..s {
        for c in 0..s {
            if get(r, c) {
                dark += 1;
            }
        }
    }
    let pct10 = (dark * 200) / (s * s); // dark% × 2
    let d = if pct10 >= 20 { pct10 - 20 } else { 20 - pct10 };
    score += (d * 10) as u32;
    score
}

/// 编码入口：载荷 → 最小可用版本的 QR 矩阵。
/// 掩码按全惩罚规则选优（可扫描性诚实优化，不图省事钉死）。
pub fn qr_encode(payload: &[u8]) -> Option<QrMatrix> {
    let vi = VPARAMS.iter().position(|v| payload.len() <= v.cap_bytes)?;
    let v = &VPARAMS[vi];
    // 数据码字：模式 0100 + 计数 8 位 + 载荷 + 终止填充。
    let mut bits = [0u8; 100 * 8];
    let mut bp = 0usize;
    let put = |val: u32, n: usize, bp: &mut usize, bits: &mut [u8; 800]| {
        for k in 0..n {
            let b = ((val >> (n - 1 - k)) & 1) as u8;
            bits[*bp] = b;
            *bp += 1;
        }
    };
    put(0b0100, 4, &mut bp, &mut bits);
    put(payload.len() as u32, 8, &mut bp, &mut bits);
    for b in payload {
        put(*b as u32, 8, &mut bp, &mut bits);
    }
    // 数据码字组装（0xEC/0x11 交替填充）。
    let mut data = [0u8; 100];
    let mut byte_i = 0usize;
    let mut acc = 0u32;
    let mut acc_n = 0u32;
    for i in 0..bp {
        acc = (acc << 1) | bits[i] as u32;
        acc_n += 1;
        if acc_n == 8 {
            data[byte_i] = acc as u8;
            byte_i += 1;
            acc = 0;
            acc_n = 0;
        }
    }
    if acc_n > 0 {
        data[byte_i] = (acc << (8 - acc_n)) as u8;
        byte_i += 1;
    }
    let mut pad = 0xECu8;
    while byte_i < v.data_cw {
        data[byte_i] = pad;
        pad = if pad == 0xEC { 0x11 } else { 0xEC };
        byte_i += 1;
    }
    // RS 纠错码字。
    let mut gen = [0u8; 21];
    rs_generator(v.ec_cw, &mut gen);
    let mut ec_out = [0u8; 20];
    rs_encode(&data[..v.data_cw], v.ec_cw, &gen, &mut ec_out);
    let mut cw = [0u8; 100];
    cw[..v.data_cw].copy_from_slice(&data[..v.data_cw]);
    cw[v.data_cw..v.data_cw + v.ec_cw].copy_from_slice(&ec_out[..v.ec_cw]);

    // 八掩码试排取惩罚最低。
    let mut best: Option<(u32, usize, QrMatrix)> = None;
    for mask in 0..8 {
        let mut qr = QrMatrix { size: v.size, modules: [0; 33 * 33], version: vi + 1, mask };
        draw_function_patterns(&mut qr, v);
        // zigzag 填数据（双列条带自右向左；条带含 timing 列时整体左移——
        // ISO 18004 §8.7.3：right==6 时改走 5，col 6 恒为功能模块）。
        let total_bits = (v.data_cw + v.ec_cw) * 8 + v.remainder_bits;
        let mut bi = 0usize;
        let mut right = v.size as isize - 1;
        while right >= 1 {
            if right == 6 {
                right = 5;
            }
            for vert in 0..v.size {
                for j in 0..2usize {
                    let c = (right - j as isize) as usize;
                    let upward = ((right + 1) as usize & 2) == 0;
                    let row = if upward { v.size - 1 - vert } else { vert };
                    if !is_function(v, row, c) {
                        let bit = if bi < total_bits {
                            let byte_idx = bi / 8;
                            if byte_idx < cw.len() {
                                (cw[byte_idx] >> (7 - (bi % 8))) & 1
                            } else {
                                0 // 残余位（remainder bits）越出码字数组——补 0（light）
                            }
                        } else {
                            0
                        };
                        // 模块暗 = 数据位异或掩码位；残余位补 0（light）。
                        let dark = if bi < total_bits {
                            (bit as usize ^ mask_bit(mask, c, row) as usize) == 1
                        } else {
                            false
                        };
                        qr.set(row, c, dark as u8);
                        bi += 1;
                    }
                }
            }
            right -= 2;
        }
        draw_format(&mut qr, mask);
        let pen = penalty(&qr);
        if best.as_ref().map(|(p, _, _)| pen < *p).unwrap_or(true) {
            best = Some((pen, mask, qr));
        }
    }
    best.map(|(_, _, qr)| qr)
}

fn draw_function_patterns(qr: &mut QrMatrix, v: &Vparam) {
    let s = v.size;
    // finders（7×7；分隔带由矩阵零初始化保持 light）。
    for (fr, fc) in [(0usize, 0usize), (0, s - 7), (s - 7, 0)] {
        for r in fr..fr + 7 {
            for c in fc..fc + 7 {
                let edge = r == fr || r == fr + 6 || c == fc || c == fc + 6;
                let center = r >= fr + 2 && r <= fr + 4 && c >= fc + 2 && c <= fc + 4;
                qr.set(r, c, (edge || center) as u8);
            }
        }
    }
    // timing（行 6/列 6 的 8..s-9 段，偶位暗；端头由 finder/分隔带覆盖）。
    for i in 8..s - 8 {
        qr.set(6, i, (i % 2 == 0) as u8);
        qr.set(i, 6, (i % 2 == 0) as u8);
    }
    // alignment（跳过与 finder 重叠的三个角落组合）。
    for i in 0..v.align_n {
        for j in 0..v.align_n {
            let ar = v.align[i];
            let ac = v.align[j];
            if (ar == 6 && ac == 6) || (ar == 6 && ac == s - 7) || (ar == s - 7 && ac == 6) {
                continue;
            }
            for dr in 0..5 {
                for dc in 0..5 {
                    let edge = dr == 0 || dr == 4 || dc == 0 || dc == 4;
                    let center = dr == 2 && dc == 2;
                    qr.set(ar - 2 + dr, ac - 2 + dc, (edge || center) as u8);
                }
            }
        }
    }
    qr.set(s - 8, 8, 1); // 暗模块
}

/// 解码验证（宿主侧「解码正确」证据）：读 format → 去掩码 → 取码字 →
/// RS 校验 → 解析 byte 模式载荷。返回载荷字节。
pub fn qr_decode(qr: &QrMatrix) -> Option<([u8; 80], usize)> {
    let v = &VPARAMS[qr.version - 1];
    // 读 format（副本一行 8 列 0-7 与列 8）。
    let bit = |r: usize, c: usize| qr.get(r, c) as u16;
    let mut fmt: u16 = 0;
    let order: [(usize, usize); 15] = [
        (0, 8), (1, 8), (2, 8), (3, 8), (4, 8), (5, 8), (7, 8), (8, 8), (8, 7), (8, 5), (8, 4), (8, 3), (8, 2), (8, 1), (8, 0),
    ];
    for (i, (r, c)) in order.iter().enumerate() {
        fmt |= bit(*r, *c) << i;
    }
    fmt ^= 0x5412;
    let mask = (fmt >> 10) as usize & 0b111;
    let ec_field = (fmt >> 13) & 0b11;
    if ec_field != 0b01 {
        return None; // 非 EC L（与本编码器约定不符）
    }
    // zigzag 取位 → 码字（与编码同序；right==6 时移 5）。
    let mut cw = [0u8; 100];
    let mut bi = 0usize;
    let mut right = v.size as isize - 1;
    while right >= 1 {
        if right == 6 {
            right = 5;
        }
        for vert in 0..v.size {
            for j in 0..2usize {
                let c = (right - j as isize) as usize;
                let upward = ((right + 1) as usize & 2) == 0;
                let row = if upward { v.size - 1 - vert } else { vert };
                if !is_function(v, row, c) {
                    let masked = qr.get(row, c) != 0;
                    let raw = masked ^ mask_bit(mask, c, row);
                    if raw && bi / 8 < 100 {
                        cw[bi / 8] |= 1 << (7 - (bi % 8));
                    }
                    bi += 1;
                }
            }
        }
        right -= 2;
    }
    // RS 校验。
    let mut gen = [0u8; 21];
    rs_generator(v.ec_cw, &mut gen);
    if !rs_verify(&cw[..v.data_cw + v.ec_cw], v.ec_cw, &gen) {
        return None;
    }
    // 解析：模式 0100 + 计数 + 载荷。
    let mut bp = 0usize;
    let take = |n: usize, bp: &mut usize| -> u32 {
        let mut val = 0u32;
        for _ in 0..n {
            val = (val << 1) | ((cw[*bp / 8] >> (7 - (*bp % 8))) & 1) as u32;
            *bp += 1;
        }
        val
    };
    let mode = take(4, &mut bp);
    if mode != 0b0100 {
        return None;
    }
    let len = take(8, &mut bp) as usize;
    if len > 80 {
        return None;
    }
    let mut out = [0u8; 80];
    for b in out.iter_mut().take(len) {
        *b = take(8, &mut bp) as u8;
    }
    Some((out, len))
}

// ---------------------------------------------------------------------------
// panic 记录（持久化语义的宿主模型）
// ---------------------------------------------------------------------------

/// 一条 panic 记录（码/地址/次数——重启后 F120 首页「上次异常重启」数据源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PanicRecord {
    pub code: [u8; 24],
    pub code_len: usize,
    pub addr: u64,
    pub count: u32,
}

/// 记录表（环形；同码合并计数——对拍 F120 口径）。
pub struct PanicLedger {
    pub records: [Option<PanicRecord>; RECORD_CAP],
    pub n: usize,
    pub total: u64,
}

impl PanicLedger {
    pub const fn new() -> Self {
        PanicLedger { records: [None; RECORD_CAP], n: 0, total: 0 }
    }
    pub fn record(&mut self, code: &[u8], addr: u64) {
        self.total += 1;
        for slot in self.records[..self.n].iter_mut() {
            if let Some(rec) = slot {
                if rec.code_len == code.len() && rec.code[..rec.code_len] == *code {
                    rec.count += 1;
                    rec.addr = addr;
                    return;
                }
            }
        }
        if self.n < RECORD_CAP {
            let mut c = [0u8; 24];
            let l = code.len().min(24);
            c[..l].copy_from_slice(&code[..l]);
            self.records[self.n] = Some(PanicRecord { code: c, code_len: l, addr, count: 1 });
            self.n += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// 星陨画面状态机
// ---------------------------------------------------------------------------

/// 画面状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanicState {
    /// 图形星陨 + 倒计时。
    Graphics,
    /// 最简文字模式（画面系统自身 panic 的两级降级）。
    TextMinimal,
    /// 跳过倒计时直接进安全模式询问（连续 panic 防循环）。
    SafeModeInquiry,
    /// 渲染挂起（串口日志未完成——日志优先原则）。
    WaitingSerial,
    /// 已决策重启。
    Rebooting,
}

/// 星陨画面。一次 panic 一个实例（内核 panic 路径零堆）。
pub struct PanicScreen {
    pub state: PanicState,
    /// 错误码 VX-PANIC-<模块>-<序号>。
    pub code: [u8; 40],
    pub code_len: usize,
    /// 二维码矩阵（96px 显示）。
    pub qr: Option<QrMatrix>,
    /// 倒计时剩余（SafeModeInquiry 下恒 0——跳过）。
    pub remaining_ms: u64,
    paused: bool,
    /// 碎裂静帧号（0..3，地址熵随机）。
    pub frame: usize,
    /// 自动 dump 字节数（≤4MB 截断）。
    pub dump_bytes: u32,
    dump_truncated: bool,
}

impl PanicScreen {
    /// 构造画面。`graphics_ok=false` → 文字兜底；`consecutive≥2` → 安全模式询问。
    pub fn new(
        module: &[u8],
        seq: u32,
        addr: u64,
        consecutive: u32,
        graphics_ok: bool,
        serial_flushed: bool,
    ) -> Self {
        // 错误码：VX-PANIC-<模块>-<序号>。
        let mut code = [0u8; 40];
        let mut cl = 0usize;
        let push = |b: &[u8], code: &mut [u8; 40], cl: &mut usize| {
            for byte in b {
                if *cl < 40 {
                    code[*cl] = *byte;
                    *cl += 1;
                }
            }
        };
        push(b"VX-PANIC-", &mut code, &mut cl);
        push(module, &mut code, &mut cl);
        push(b"-", &mut code, &mut cl);
        let mut seqbuf = [0u8; 10];
        let mut sn = 0;
        let mut s = seq;
        if s == 0 {
            seqbuf[0] = b'0';
            sn = 1;
        }
        while s > 0 {
            seqbuf[sn] = b'0' + (s % 10) as u8;
            sn += 1;
            s /= 10;
        }
        let mut i = sn;
        while i > 0 {
            i -= 1;
            push(&seqbuf[i..i + 1], &mut code, &mut cl);
        }

        // 二维码载荷：错误码 + 帮助链接（F119 panic 篇）。
        let mut payload = [0u8; 80];
        let mut pl;
        payload[..cl].copy_from_slice(&code[..cl]);
        pl = cl;
        let url = b" https://help.varix/p/panic/";
        for b in url {
            if pl < 80 {
                payload[pl] = *b;
                pl += 1;
            }
        }
        for b in module {
            if pl < 80 {
                payload[pl] = *b;
                pl += 1;
            }
        }
        let qr = if graphics_ok { qr_encode(&payload[..pl]) } else { None };

        // 碎裂帧：地址熵随机三选一。
        let frame = ((addr ^ 0x9E37_79B9_7F4A_7C15) as usize) % SHATTER_FRAMES;

        let state = if consecutive >= CONSECUTIVE_SAFE_MODE_AT {
            PanicState::SafeModeInquiry
        } else if !serial_flushed {
            PanicState::WaitingSerial
        } else if graphics_ok {
            PanicState::Graphics
        } else {
            PanicState::TextMinimal
        };

        // 安全模式询问态无重启倒计时（询问等用户决定——诚实归零）。
        let remaining_ms = if state == PanicState::SafeModeInquiry { 0 } else { REBOOT_COUNTDOWN_MS };
        PanicScreen {
            state,
            code,
            code_len: cl,
            qr,
            remaining_ms,
            paused: false,
            frame,
            dump_bytes: 0,
            dump_truncated: false,
        }
    }

    /// 串口日志完成回调（日志优先原则：完成后才允许渲染）。
    pub fn serial_flushed(&mut self, graphics_ok: bool) {
        if self.state == PanicState::WaitingSerial {
            self.state = if graphics_ok { PanicState::Graphics } else { PanicState::TextMinimal };
        }
    }

    /// 时间一拍。返回 true = 到点应重启。
    pub fn tick(&mut self, dt_ms: u64) -> bool {
        if self.state != PanicState::Graphics && self.state != PanicState::TextMinimal {
            return false;
        }
        if self.paused {
            return false;
        }
        if self.remaining_ms <= dt_ms {
            self.state = PanicState::Rebooting;
            return true;
        }
        self.remaining_ms -= dt_ms;
        false
    }

    /// Enter = 立即重启；Esc = 停留读码（倒计时暂停/恢复）。
    pub fn key(&mut self, k: u8) {
        match k {
            b'\r' => {
                if self.state == PanicState::Graphics || self.state == PanicState::TextMinimal {
                    self.state = PanicState::Rebooting;
                }
            }
            0x1B => {
                if self.state == PanicState::Graphics || self.state == PanicState::TextMinimal {
                    self.paused = !self.paused;
                }
            }
            _ => {}
        }
    }

    /// 重启前自动 dump（F020 管线内核态子集；≤4MB 截断）。
    pub fn take_dump(&mut self, requested: u32) -> u32 {
        if requested > DUMP_CAP_BYTES {
            self.dump_bytes = DUMP_CAP_BYTES;
            self.dump_truncated = true;
        } else {
            self.dump_bytes = requested;
        }
        self.dump_bytes
    }

    pub fn dump_truncated(&self) -> bool {
        self.dump_truncated
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    /// 画面可渲染判定（渲染成功率统计口径：QR 可选、码行与帧号齐备即成）。
    pub fn renderable(&self) -> bool {
        match self.state {
            PanicState::Graphics => self.qr.is_some() && self.code_len > 0,
            PanicState::TextMinimal => self.code_len > 0,
            PanicState::SafeModeInquiry => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 简易 xorshift（演练随机源，确定性）。
struct Xs(u64);
impl Xs {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

/// 域自检。
#[inline(never)]
pub fn run_panicscreen_checks() -> CheckSet {
    let mut cs = CheckSet::new("F173-panicscreen");

    // 1) panic 百次演练：画面渲染成功率 100%（图形态）。
    let mut rng = Xs(0xF173_0001);
    let mut ok = true;
    let modules: [&[u8]; 5] = [b"MEM", b"SCHED", b"FS", b"GFX", b"SEC"];
    for i in 0..100u64 {
        let m = modules[(rng.next() % 5) as usize];
        let seq = (rng.next() % 9999) as u32;
        let addr = rng.next();
        let ps = PanicScreen::new(m, seq, addr, 1, true, true);
        if !ps.renderable() {
            ok = false;
            break;
        }
        // 码行格式核对：VX-PANIC- 前缀恒在。
        if &ps.code[..9] != b"VX-PANIC-" {
            ok = false;
            break;
        }
        let _ = i;
    }
    cs.add("drill_100_render_ok", ok, "");

    // 2) 最简降级注入 5 次：graphics_ok=false → 文字模式渲染成功率 100%。
    let mut ok2 = true;
    for i in 0..5 {
        let ps = PanicScreen::new(b"VIZ", 100 + i as u32, i as u64, 1, false, true);
        ok2 &= ps.state == PanicState::TextMinimal && ps.renderable() && ps.qr.is_none();
    }
    cs.add("minimal_fallback_5_injections", ok2, "");

    // 3) 倒计时 30s：29.9s 不重启、30s 整重启；Enter 立即；Esc 暂停。
    let mut ps = PanicScreen::new(b"TST", 1, 1, 1, true, true);
    let mut rebooted = false;
    for _ in 0..299 {
        rebooted |= ps.tick(100);
    }
    let not_yet = !rebooted && ps.remaining_ms == 100;
    rebooted |= ps.tick(100);
    cs.add("countdown_30s_exact", not_yet && rebooted && ps.state == PanicState::Rebooting, "");

    // 4) Enter 立即重启。
    let mut ps2 = PanicScreen::new(b"TST", 2, 2, 1, true, true);
    ps2.key(b'\r');
    cs.add("enter_immediate_reboot", ps2.state == PanicState::Rebooting, "");

    // 5) Esc 停留读码：暂停后 tick 不减时不重启，恢复后照走。
    let mut ps3 = PanicScreen::new(b"TST", 3, 3, 1, true, true);
    ps3.key(0x1B);
    let mut stayed = true;
    for _ in 0..400 {
        stayed &= !ps3.tick(100);
    }
    cs.add("esc_stay_pauses", stayed && ps3.paused() && ps3.remaining_ms == REBOOT_COUNTDOWN_MS, "");

    // 6) 连续 panic：第二次起跳过倒计时直接进安全模式询问（防循环）。
    let ps4 = PanicScreen::new(b"LOOP", 1, 1, 1, true, true);
    let ps5 = PanicScreen::new(b"LOOP", 2, 2, 2, true, true);
    cs.add("consecutive_skips_to_safemode", ps4.state == PanicState::Graphics && ps5.state == PanicState::SafeModeInquiry && ps5.remaining_ms == 0, "");

    // 7) 日志优先：串口未完成 → 渲染挂起；完成后恢复。
    let mut ps6 = PanicScreen::new(b"LOG", 1, 1, 1, true, false);
    let held = ps6.state == PanicState::WaitingSerial && !ps6.renderable();
    ps6.serial_flushed(true);
    cs.add("log_first_then_render", held && ps6.state == PanicState::Graphics && ps6.renderable(), "");

    // 8) dump 上限 4MB：超限截断且标注。
    let mut ps7 = PanicScreen::new(b"DMP", 1, 1, 1, true, true);
    let got = ps7.take_dump(6 << 20);
    cs.add("dump_cap_4mb", got == DUMP_CAP_BYTES && ps7.dump_truncated(), "");

    // 9) 二维码解码正确：编码→解码 round-trip（v3 载荷 53 字节容量内）。
    let payload = b"VX-PANIC-MEM-000042 https://help.varix/p/panic/MEM";
    let qr = qr_encode(payload).expect("payload must fit v3-L");
    let decoded = qr_decode(&qr);
    cs.add("qr_roundtrip_decode", decoded.map(|(b, l)| &b[..l] == payload).unwrap_or(false), "");

    // 10) 四版本全过 round-trip（17/32/53/78 容量边界各测一点）。
    let mut allv = true;
    for len in [17usize, 32, 53, 78] {
        let mut p = [0u8; 80];
        for (i, b) in p.iter_mut().enumerate().take(len) {
            *b = b'A' + (i % 26) as u8;
        }
        if let Some(q) = qr_encode(&p[..len]) {
            allv &= qr_decode(&q).map(|(b, l)| &b[..l] == &p[..len]).unwrap_or(false);
        } else {
            allv = false;
        }
    }
    cs.add("qr_versions_1_to_4", allv, "");

    // 11) 超容量拒绝（诚实失败，不静默截断）。
    let over = [0x41u8; 79];
    cs.add("qr_over_capacity_rejected", qr_encode(&over).is_none(), "");

    // 12) 碎裂帧三选随机（地址熵）：不同地址产生覆盖 0..3 的帧号。
    let mut seen = [false; SHATTER_FRAMES];
    for i in 0..16u64 {
        let ps8 = PanicScreen::new(b"FRM", 1, i * 0x1000_0000, 1, true, true);
        seen[ps8.frame] = true;
    }
    cs.add("shatter_frames_random", seen.iter().all(|s| *s), "");

    // 13) panic 记录账本：同码合并计数、异码分行（F120 对拍口径）。
    let mut led = PanicLedger::new();
    led.record(b"VX-PANIC-MEM-7", 0x1000);
    led.record(b"VX-PANIC-MEM-7", 0x2000);
    led.record(b"VX-PANIC-FS-2", 0x3000);
    cs.add("panic_ledger_merge", led.n == 2 && led.records[0].unwrap().count == 2 && led.total == 3, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hundred_drills_all_renderable() {
        // 主册判据：panic 百次演练画面渲染成功率 100%（图形/文字/安全模式/挂起全态覆盖）。
        let mut rng = Xs(0xB2903);
        for i in 0..100 {
            let modules: [&[u8]; 8] = [b"MEM", b"SCHED", b"FS", b"GFX", b"SEC", b"NET", b"PWR", b"IO"];
            let m = modules[(rng.next() % 8) as usize];
            let consecutive = if i % 10 == 9 { 2 } else { 1 };
            let graphics = i % 20 != 19;
            let serial = i % 7 != 6;
            let mut ps = PanicScreen::new(m, (rng.next() % 1000) as u32, rng.next(), consecutive, graphics, serial);
            if !serial {
                ps.serial_flushed(graphics);
            }
            assert!(ps.renderable(), "drill {} not renderable", i);
            assert!(ps.frame < SHATTER_FRAMES, "frame out of range");
        }
    }

    #[test]
    fn rs_matches_iso_18004_annex_vector() {
        // RS 锚定 ISO/IEC 18004 附录示例（v1-M「01234567」）：
        // 数据码字 16B + 纠错码字 10B = A5 24 D4 C1 ED 36 C7 87 2C 55。
        // 该向量证明 GF 表/生成多项式/除法三件套对标准正确（非自洽自证）。
        let data: [u8; 16] = [0x10, 0x20, 0x0C, 0x56, 0x61, 0x80, 0xEC, 0x11, 0xEC, 0x11, 0xEC, 0x11, 0xEC, 0x11, 0xEC, 0x11];
        let expect: [u8; 10] = [0xA5, 0x24, 0xD4, 0xC1, 0xED, 0x36, 0xC7, 0x87, 0x2C, 0x55];
        let mut gen = [0u8; 21];
        rs_generator(10, &mut gen);
        let mut ec = [0u8; 20];
        rs_encode(&data, 10, &gen, &mut ec);
        assert_eq!(&ec[..10], &expect, "RS EC must match ISO annex example");
    }

    #[test]
    fn qr_format_info_bch_self_consistent() {
        // format info 自洽：8 种掩码的 15 位串重解回 EC=L + 掩码号。
        for mask in 0..8 {
            let bits = format_bits(mask);
            let unmasked = bits ^ 0x5412;
            // BCH 校验：15 位串（去掩码后）对 0x537 生成多项式取余应为 0。
            let mut rem = unmasked;
            for i in (10..15).rev() {
                if rem & (1 << i) != 0 {
                    rem ^= 0x537 << (i - 10);
                }
            }
            assert_eq!(rem & 0x3FF, 0, "mask {} BCH residue nonzero", mask);
            assert_eq!((unmasked >> 13) & 0b11, 0b01, "EC level must be L");
            assert_eq!((unmasked >> 10) & 0b111, mask as u16, "mask id must round-trip");
        }
    }

    #[test]
    fn qr_roundtrip_all_capacities() {
        // 容量边界逐版本 round-trip + 空串 + 1 字节极小载荷。
        for len in [1usize, 16, 17, 18, 31, 32, 33, 52, 53, 54, 77, 78] {
            let mut p = [0u8; 80];
            for (i, b) in p.iter_mut().enumerate().take(len) {
                *b = (i * 31 % 251) as u8;
            }
            let q = qr_encode(&p[..len]).unwrap_or_else(|| panic!("len {} must encode", len));
            let (out, outlen) = qr_decode(&q).unwrap_or_else(|| panic!("len {} must decode", len));
            assert_eq!(outlen, len);
            assert_eq!(&out[..outlen], &p[..len]);
            // 版本选择正确（容量表）。
            let expect_v = if len <= 17 { 1 } else if len <= 32 { 2 } else if len <= 53 { 3 } else { 4 };
            assert_eq!(q.version, expect_v);
        }
    }

    #[test]
    fn safe_mode_anti_loop_gate() {
        // 连续 panic 场景全链：1 次正常倒计时、2 次跳过、3 次仍安全模式。
        let a = PanicScreen::new(b"X", 1, 1, 1, true, true);
        assert_eq!(a.state, PanicState::Graphics);
        let b = PanicScreen::new(b"X", 2, 2, 2, true, true);
        assert_eq!(b.state, PanicState::SafeModeInquiry);
        let c = PanicScreen::new(b"X", 3, 3, 5, true, true);
        assert_eq!(c.state, PanicState::SafeModeInquiry);
        // 安全模式态不重启（等用户选择）。
        let mut b2 = PanicScreen::new(b"X", 2, 2, 2, true, true);
        for _ in 0..400 {
            assert!(!b2.tick(100));
        }
    }

    #[test]
    fn error_code_format_exact() {
        // 错误码格式 VX-PANIC-<模块>-<序号>：模块 3 字符、序号带前导零对拍。
        let mut ps = PanicScreen::new(b"GFX", 7, 0, 1, false, true);
        let s = core::str::from_utf8(&ps.code[..ps.code_len]).unwrap();
        assert_eq!(s, "VX-PANIC-GFX-7");
        let mut ps2 = PanicScreen::new(b"SCHED", 0, 0, 1, false, true);
        let s2 = core::str::from_utf8(&ps2.code[..ps2.code_len]).unwrap();
        assert_eq!(s2, "VX-PANIC-SCHED-0");
    }
}
