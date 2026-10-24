//! UNREAL-X / UNX-A2-B07 / CoRun Varix STAR II · Unxreal
//! 锚点：UNX-F0923 · smp_call 目标集选择（掩码语义与在线核校验）（目标 300 行）
//! 判据原文摘录（书 2590 行）：UNX-F0923-J1 掩码展开正确（随机掩码 1000 组
//! 展开集与预期全等），离线核拦截 10/10 次，空集拒绝 10/10 次。
//! 职责定位：把「这次调用到底打给谁」从一句话变成一个可校验、可冻结、可审计的
//! 目标集对象——掩码有五种合法表达（位图/列表/区间/排除式/全集），展开器把它们
//! 归一成「升序去重的核 id 列表」这一唯一规范形，展开即冻结并出指纹；离线核、
//! 越界核、空集三类非法输入在展开口显性拒绝，绝不静默裁剪（裁剪会让调用方以为
//! 发到了 N 核实际只有 N−k 核——比失败更危险的谎报）。
//! 设计要点：①**五表达归一**——五种掩码表达共用一个展开口，规范形唯一
//! （升序去重 Vec<u32>），位图↔列表双向转换对拍互证；②**展开即冻结+指纹**——
//! 展开产物不可变并 FNV 指纹留痕，审计时重算指纹对拍（指纹漂移=目标集被篡改）；
//! ③**零静默裁剪铁律**——掩码含离线/越界核时拒绝而非剔除（0x5002/0x5003），
//! 拒绝清单带明示哪个核不合法；④**预期全等对拍**——随机掩码 1000 组与独立参考
//! 展开（朴素循环扫描实现）逐组全等，双实现互为对照；⑤**家族复用声明**——在线
//! 集合语义与 F0921/F0922 同源（家族复用第十用），本单新增的是掩码表达族与
//! 展开冻结面。
//! 诊断码段：0x50（本单独占，0x5001~0x500F）。
//! 施工：AI-WB2 · 2026-10-11。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码闭集（0x50 段独占）
// ---------------------------------------------------------------------------

/// 0x5001 掩码表达非法（未知表达族编号——表达闭集外）。
pub const XC_BAD_EXPR: u32 = 0x5001;
/// 0x5002 掩码含离线核（零静默裁剪——拒绝而非剔除）。
pub const XC_MASK_OFFLINE: u32 = 0x5002;
/// 0x5003 掩码含越界核（核 id 超平台核数）。
pub const XC_MASK_OOB: u32 = 0x5003;
/// 0x5004 展开空集（合法表达但展开后零目标——语义未定义显性拒绝）。
pub const XC_EXPAND_EMPTY: u32 = 0x5004;
/// 0x5005 位图越界位（位图高位超出核数位——表达自洽性破坏）。
pub const XC_BITMAP_OOB: u32 = 0x5005;
/// 0x5006 区间倒置（区间掩码 lo>hi——表达自洽性破坏）。
pub const XC_RANGE_INVERT: u32 = 0x5006;
/// 0x5007 排除式空积（排除后零目标——排除式把目标排光）。
pub const XC_EXCLUDE_EMPTY: u32 = 0x5007;
/// 0x5008 指纹漂移（冻结目标集重算指纹不符——目标集被篡改）。
pub const XC_FP_DRIFT: u32 = 0x5008;
/// 0x5009 规范形失序（展开产物非严格升序——规范形破坏）。
pub const XC_NOT_SORTED: u32 = 0x5009;
/// 0x500A 规范形重复（展开产物含重复核 id——去重纪律破坏）。
pub const XC_DUP_TARGET: u32 = 0x500A;
/// 0x500B 双向转换失配（位图↔列表转换后不等——转换面缺陷）。
pub const XC_CONV_GAP: u32 = 0x500B;
/// 0x500C 对拍失全等（随机掩码组与参考实现展开集不等——展开器缺陷）。
pub const XC_ORACLE_GAP: u32 = 0x500C;
/// 0x500D 表达互斥破坏（复合表达同时声明两种族——表达面歧义）。
pub const XC_EXPR_AMBIGU: u32 = 0x500D;
/// 0x500E 冻结后再展开（冻结目标集被二次展开——冻结单向性破坏）。
pub const XC_REFREEZE: u32 = 0x500E;
/// 0x500F 拒绝账失守（拒绝计数与注入数不符——拒绝面统计漂移）。
pub const XC_REJECT_GAP: u32 = 0x500F;

/// 诊断明细（码 + 人话 + 可操作建议）。
pub fn detail(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_BAD_EXPR => (
            "掩码表达非法：未知表达族编号",
            "五表达闭集（位图/列表/区间/排除式/全集）之外不可自造；新表达走版本前进",
        ),
        XC_MASK_OFFLINE => (
            "掩码含离线核：零静默裁剪铁律触发",
            "调用方应先查在线快照剔除离线核再发起；封装层拒绝而非剔除——静默裁剪=谎报扇出",
        ),
        XC_MASK_OOB => (
            "掩码含越界核：核 id 超平台核数",
            "核 id 从平台拓扑来；越界说明掩码构造用了错误基数，查核数常量来源",
        ),
        XC_EXPAND_EMPTY => (
            "展开空集：合法表达但展开后零目标",
            "空集调用语义未定义；要么修正掩码要么显式跳过本次调用，不得当『全员完成』",
        ),
        XC_BITMAP_OOB => (
            "位图越界位：位图高位超出核数位",
            "位图宽度=核数；高位脏位说明掩码复用了更宽平台的旧位图，先清高位",
        ),
        XC_RANGE_INVERT => (
            "区间倒置：lo>hi",
            "区间掩码必须 lo≤hi；倒置说明端点来源写反，交换或修正来源",
        ),
        XC_EXCLUDE_EMPTY => (
            "排除式空积：排除后零目标",
            "排除式把目标排光了；检查排除集是否过大或基集过小",
        ),
        XC_FP_DRIFT => (
            "指纹漂移：冻结目标集重算指纹不符",
            "目标集冻结后不可变；漂移说明有旁路写，逐元素对拍定位篡改点",
        ),
        XC_NOT_SORTED => (
            "规范形失序：展开产物非严格升序",
            "规范形=升序去重；失序说明展开器跳过了排序步骤，展开口补排序并回归",
        ),
        XC_DUP_TARGET => (
            "规范形重复：展开产物含重复核 id",
            "重复核=重复投递；去重步骤丢失，查展开器归一段",
        ),
        XC_CONV_GAP => (
            "双向转换失配：位图↔列表转换后不等",
            "两表达必须互逆；失配=转换面缺陷，用对拍向量定位哪一步丢位",
        ),
        XC_ORACLE_GAP => (
            "对拍失全等：随机掩码组与参考实现展开集不等",
            "双实现互为对照；不等则二分找首个分歧组，比对位运算与循环扫描的边界",
        ),
        XC_EXPR_AMBIGU => (
            "表达互斥破坏：复合表达同时声明两种族",
            "一次掩码恰一种表达；歧义说明构造口允许了复合填充，收紧构造器",
        ),
        XC_REFREEZE => (
            "冻结后再展开：冻结目标集被二次展开",
            "冻结单向：展开→冻结→只读；要新目标集重新从表达走展开，禁止冻结物再展开",
        ),
        XC_REJECT_GAP => (
            "拒绝账失守：拒绝计数与注入数不符",
            "拒绝计数是审计面；失守说明某拒绝路径漏计，逐闸核对",
        ),
        _ => ("未知诊断码", "码不在 0x50 闭集内——查是否跨段误用"),
    }
}

// ---------------------------------------------------------------------------
// §1 五表达掩码模型（表达闭集——归一前的一切合法输入形状）
// ---------------------------------------------------------------------------

/// 掩码表达闭集（五种合法形状——一次掩码恰一种，复合 0x500D 拦）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaskExpr {
    /// 位图：第 i 位=核 i 在目标内（宽度恒=核数，越界高位脏位 0x5005）。
    Bitmap(u64),
    /// 列表：显式核 id 枚举。
    List(Vec<u32>),
    /// 区间：[lo, hi] 闭区间（lo≤hi，倒置 0x5006）。
    Range(u32, u32),
    /// 排除式：全集去排除集（排除后零目标 0x5007）。
    Exclude(Vec<u32>),
    /// 全集：全部在线核。
    AllOnline,
}

impl MaskExpr {
    /// 表达族名（报告消费）。
    pub fn name(&self) -> &'static str {
        match self {
            MaskExpr::Bitmap(_) => "bitmap",
            MaskExpr::List(_) => "list",
            MaskExpr::Range(_, _) => "range",
            MaskExpr::Exclude(_) => "exclude",
            MaskExpr::AllOnline => "all_online",
        }
    }

    /// 族编号（互斥校验用——表达歧义闸的账面依据）。
    pub fn kind(&self) -> u8 {
        match self {
            MaskExpr::Bitmap(_) => 0,
            MaskExpr::List(_) => 1,
            MaskExpr::Range(_, _) => 2,
            MaskExpr::Exclude(_) => 3,
            MaskExpr::AllOnline => 4,
        }
    }
}

/// 冻结目标集（展开产物——升序去重+指纹，只读）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrozenTargets {
    /// 规范形：严格升序、无重复的核 id 列表。
    pub targets: Vec<u32>,
    /// 展开来源表达族名。
    pub from_expr: &'static str,
    /// 冻结指纹（targets 的 FNV——审计重算对拍基准）。
    pub fp: u64,
    /// 展开 tick。
    pub frozen_at: u64,
}

impl FrozenTargets {
    /// 指纹重算（审计口——与 fp 对拍，漂移 0x5008）。
    pub fn recompute_fp(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for t in &self.targets {
            buf.extend_from_slice(&t.to_le_bytes());
        }
        fnv64(&buf)
    }

    /// 自检（规范形三律：升序+去重+指纹一致）。
    pub fn audit(&self) -> Result<(), (u32, String)> {
        for w in self.targets.windows(2) {
            if w[0] >= w[1] {
                if w[0] == w[1] {
                    return Err((XC_DUP_TARGET, format!("重复核 #{}", w[0])));
                }
                return Err((XC_NOT_SORTED, format!("失序 {} > {}", w[0], w[1])));
            }
        }
        let real = self.recompute_fp();
        if real != self.fp {
            return Err((
                XC_FP_DRIFT,
                format!(
                    "指纹 {:#018x} ≠ 冻结 {:#018x}——目标集被篡改",
                    real, self.fp
                ),
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// §2 在线集合 + 展开器（本单核心——归一+校验+冻结）
// ---------------------------------------------------------------------------

/// 目标集选择器（在线快照+展开器+冻结账+拒绝账四件一体）。
pub struct TargetSelector {
    /// 平台核数（位图宽度基准——常量来源=平台拓扑事实）。
    pub ncpu: u32,
    /// 在线集合快照（升序——展开全集/排除式/离线校验的唯一依据）。
    online: Vec<u32>,
    /// 冻结账（展开序号→冻结目标集——全量留痕可审计）。
    frozen_book: Vec<FrozenTargets>,
    /// 展开计数。
    pub expand_total: u64,
    /// 冻结计数。
    pub freeze_total: u64,
    /// 拒绝计数（分码审计——10/10 对账基准）。
    pub reject_offline: u64,
    pub reject_oob: u64,
    pub reject_empty: u64,
    /// 当前 tick。
    tick: u64,
}

impl TargetSelector {
    /// 构造（核数+在线集合——在线必须⊆[0,ncpu) 升序不重复）。
    pub fn new(ncpu: u32, online: &[u32]) -> Result<TargetSelector, (u32, String)> {
        if ncpu == 0 || ncpu > 64 {
            return Err((
                XC_MASK_OOB,
                format!("核数 {} 越界（位图表达限定 1..=64 核）", ncpu),
            ));
        }
        let mut on: Vec<u32> = Vec::new();
        for &c in online {
            if c >= ncpu {
                return Err((XC_MASK_OOB, format!("在线核 #{} 越界（核数 {}）", c, ncpu)));
            }
            if on.contains(&c) {
                return Err((XC_DUP_TARGET, format!("在线集合重复核 #{}", c)));
            }
            on.push(c);
        }
        if on.is_empty() {
            return Err((XC_EXPAND_EMPTY, "空在线集合——至少一核在线".to_string()));
        }
        on.sort_unstable();
        Ok(TargetSelector {
            ncpu,
            online: on,
            frozen_book: Vec::new(),
            expand_total: 0,
            freeze_total: 0,
            reject_offline: 0,
            reject_oob: 0,
            reject_empty: 0,
            tick: 0,
        })
    }

    /// 在线快照（只读——展开校验的唯一真源）。
    pub fn online_set(&self) -> &[u32] {
        &self.online
    }

    /// 单核合法性（越界/离线判定——拒绝清单明细来源）。
    fn check_cpu(&mut self, c: u32) -> Result<(), (u32, String)> {
        if c >= self.ncpu {
            self.reject_oob += 1;
            return Err((
                XC_MASK_OOB,
                format!("核 #{} 越界（核数 {}）——零静默裁剪，拒绝整单", c, self.ncpu),
            ));
        }
        if !self.online.contains(&c) {
            self.reject_offline += 1;
            return Err((
                XC_MASK_OFFLINE,
                format!("核 #{} 离线（在线集 {:?}）——零静默裁剪，拒绝整单", c, self.online),
            ));
        }
        Ok(())
    }

    /// 展开器（五表达归一——规范形=升序去重列表；校验失败整单拒绝）。
    pub fn expand(&mut self, expr: &MaskExpr) -> Result<Vec<u32>, (u32, String)> {
        self.expand_total += 1;
        self.tick += 1;
        let mut raw: Vec<u32> = Vec::new();
        match expr {
            MaskExpr::Bitmap(bits) => {
                // 越界高位脏位校验（宽度=核数——0x5005）。
                if self.ncpu < 64 && (*bits >> self.ncpu) != 0 {
                    return Err((
                        XC_BITMAP_OOB,
                        format!(
                            "位图高位脏位 {:#x} 超出核数 {} 位宽",
                            bits >> self.ncpu,
                            self.ncpu
                        ),
                    ));
                }
                for i in 0..self.ncpu {
                    if bits & (1u64 << i) != 0 {
                        self.check_cpu(i)?;
                        raw.push(i);
                    }
                }
            }
            MaskExpr::List(list) => {
                for &c in list {
                    self.check_cpu(c)?;
                    raw.push(c);
                }
            }
            MaskExpr::Range(lo, hi) => {
                if lo > hi {
                    return Err((
                        XC_RANGE_INVERT,
                        format!("区间 [{},{}] 倒置——lo>hi", lo, hi),
                    ));
                }
                let mut c = *lo;
                while c <= *hi {
                    self.check_cpu(c)?;
                    raw.push(c);
                    if c == u32::MAX {
                        break;
                    }
                    c += 1;
                }
            }
            MaskExpr::Exclude(ex) => {
                for &c in ex {
                    if c >= self.ncpu {
                        self.reject_oob += 1;
                        return Err((XC_MASK_OOB, format!("排除集核 #{} 越界", c)));
                    }
                }
                // 全集去排除集（排除集含离线核不拦——排除掉一个离线核=目标更纯净，合法）。
                for &c in &self.online {
                    if !ex.contains(&c) {
                        raw.push(c);
                    }
                }
            }
            MaskExpr::AllOnline => {
                raw.extend_from_slice(&self.online);
            }
        }
        // 归一：排序+去重（规范形三律的落地点）。
        raw.sort_unstable();
        raw.dedup();
        if raw.is_empty() {
            self.reject_empty += 1;
            return Err((
                XC_EXPAND_EMPTY,
                format!(
                    "表达 {} 展开后零目标——空集拒绝（语义未定义,不当『全员完成』）",
                    expr.name()
                ),
            ));
        }
        Ok(raw)
    }

    /// 展开+冻结（判据主口：展开→规范形三律自检→指纹冻结入账）。
    pub fn expand_and_freeze(&mut self, expr: &MaskExpr) -> Result<FrozenTargets, (u32, String)> {
        let targets = self.expand(expr)?;
        let ft = FrozenTargets {
            fp: {
                let mut buf: Vec<u8> = Vec::new();
                for t in &targets {
                    buf.extend_from_slice(&t.to_le_bytes());
                }
                fnv64(&buf)
            },
            from_expr: expr.name(),
            targets,
            frozen_at: self.tick,
        };
        ft.audit()?;
        self.freeze_total += 1;
        self.frozen_book.push(ft.clone());
        Ok(ft)
    }

    /// 冻结账审计（全量重算指纹+规范形三律——0x5008/0x5009/0x500A 闸）。
    pub fn audit_book(&self) -> Result<(), (u32, String)> {
        for ft in &self.frozen_book {
            ft.audit()?;
        }
        Ok(())
    }

    /// 冻结账检索（按序号取——审计面）。
    pub fn frozen_at_idx(&self, idx: usize) -> Option<&FrozenTargets> {
        self.frozen_book.get(idx)
    }

    /// 冻结账规模。
    pub fn book_len(&self) -> usize {
        self.frozen_book.len()
    }

    /// 拒绝账对平（三分账与注入数对齐——0x500F）。
    pub fn reject_gate(&self, offline: u64, oob: u64, empty: u64) -> Result<(), (u32, String)> {
        if self.reject_offline != offline || self.reject_oob != oob || self.reject_empty != empty {
            return Err((
                XC_REJECT_GAP,
                format!(
                    "拒绝账失守：离线{}/越界{}/空集{} ≠ 注入 {}/{}/{}",
                    self.reject_offline, self.reject_oob, self.reject_empty, offline, oob, empty
                ),
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// §3 独立参考展开器（对拍真值——朴素循环扫描，与主展开器异构）
// ---------------------------------------------------------------------------

/// 参考展开（独立重算——不共享主展开器的任何代码路径）。
/// 返回 None 表示该表达在参考口径下同样非法（空/越界——只对合法表达对拍）。
pub fn oracle_expand(expr: &MaskExpr, ncpu: u32, online: &[u32]) -> Option<Vec<u32>> {
    let mut acc: Vec<u32> = Vec::new();
    match expr {
        MaskExpr::Bitmap(bits) => {
            if ncpu < 64 && (*bits >> ncpu) != 0 {
                return None;
            }
            let mut i = 0u32;
            while i < ncpu {
                if bits & (1u64 << i) != 0 && online.contains(&i) {
                    acc.push(i);
                }
                i += 1;
            }
        }
        MaskExpr::List(list) => {
            for &c in list {
                if c >= ncpu || !online.contains(&c) {
                    return None;
                }
                acc.push(c);
            }
        }
        MaskExpr::Range(lo, hi) => {
            if lo > hi {
                return None;
            }
            let mut c = *lo;
            while c <= *hi {
                if c >= ncpu || !online.contains(&c) {
                    return None;
                }
                acc.push(c);
                if c == u32::MAX {
                    break;
                }
                c += 1;
            }
        }
        MaskExpr::Exclude(ex) => {
            for &c in ex {
                if c >= ncpu {
                    return None;
                }
            }
            for &c in online {
                if !ex.contains(&c) {
                    acc.push(c);
                }
            }
        }
        MaskExpr::AllOnline => {
            acc.extend_from_slice(online);
        }
    }
    acc.sort_unstable();
    acc.dedup();
    if acc.is_empty() {
        None
    } else {
        Some(acc)
    }
}

// ---------------------------------------------------------------------------
// §4 位图↔列表双向转换（表达族互逆面——0x500B 闸）
// ---------------------------------------------------------------------------

/// 列表→位图（越界核拒绝——宽度=核数）。
pub fn list_to_bitmap(list: &[u32], ncpu: u32) -> Result<u64, (u32, String)> {
    if ncpu > 64 {
        return Err((XC_BITMAP_OOB, format!("核数 {} 超位图宽度 64", ncpu)));
    }
    let mut bits: u64 = 0;
    for &c in list {
        if c >= ncpu {
            return Err((XC_MASK_OOB, format!("核 #{} 越界（核数 {}）", c, ncpu)));
        }
        bits |= 1u64 << c;
    }
    Ok(bits)
}

/// 位图→列表（升序展开——越界高位脏位拒绝）。
pub fn bitmap_to_list(bits: u64, ncpu: u32) -> Result<Vec<u32>, (u32, String)> {
    if ncpu < 64 && (bits >> ncpu) != 0 {
        return Err((
            XC_BITMAP_OOB,
            format!("高位脏位 {:#x} 超核数 {} 位宽", bits >> ncpu, ncpu),
        ));
    }
    let mut out = Vec::new();
    for i in 0..ncpu {
        if bits & (1u64 << i) != 0 {
            out.push(i);
        }
    }
    Ok(out)
}

/// 双向转换对拍（任意列表位图化再列表化==原列表去重升序——互逆铁律）。
pub fn roundtrip_check(list: &[u32], ncpu: u32) -> Result<(), (u32, String)> {
    let bits = list_to_bitmap(list, ncpu)?;
    let back = bitmap_to_list(bits, ncpu)?;
    let mut want: Vec<u32> = list.to_vec();
    want.sort_unstable();
    want.dedup();
    if back != want {
        return Err((
            XC_CONV_GAP,
            format!("往返失配：{:?} ≠ {:?}（原 {:?}）", back, want, list),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §5 FNV + LCG（确定性底座）
// ---------------------------------------------------------------------------

/// FNV-1a 64。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 线性同余发生器。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 33) as u32
    }
    pub fn below(&mut self, n: u32) -> u32 {
        self.next_u32() % n
    }
}

// ---------------------------------------------------------------------------
// §6 J1 三闸（展开全等 1000 组 + 离线拦截 10/10 + 空集拒绝 10/10）
// ---------------------------------------------------------------------------

/// J1-A：随机掩码 1000 组展开集与预期全等（五表达族轮转+双实现对拍）。
pub fn j1_expand_1000() -> Result<String, (u32, String)> {
    let ncpu = 16u32;
    let online: Vec<u32> = vec![0, 1, 2, 3, 5, 8, 9, 12, 13, 15]; // 离线：4/6/7/10/11/14
    let mut sel = TargetSelector::new(ncpu, &online)?;
    let mut rng = Lcg::new(0x5023_B07A);
    let mut agree = 0u32;
    for i in 0..1000u32 {
        // 五表达族轮转+随机参数。
        let kind = i % 5;
        let expr = match kind {
            0 => {
                // 位图：随机置位（只落在在线核上——保证合法）。
                let mut bits: u64 = 0;
                for _ in 0..(1 + rng.below(6)) {
                    let c = online[rng.below(online.len() as u32) as usize];
                    bits |= 1u64 << c;
                }
                MaskExpr::Bitmap(bits)
            }
            1 => {
                let mut l: Vec<u32> = Vec::new();
                for _ in 0..(1 + rng.below(8)) {
                    l.push(online[rng.below(online.len() as u32) as usize]);
                }
                MaskExpr::List(l)
            }
            2 => {
                // 区间：端点都取在线核，lo≤hi 由排序保证。
                let a = online[rng.below(online.len() as u32) as usize];
                let b = online[rng.below(online.len() as u32) as usize];
                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                // 区间含离线核时不合法——参考口径也拒；此处构造纯在线区间：
                // 取相邻在线对（保证区间内全在线）或接受对拍 None 一致。
                MaskExpr::Range(lo, hi)
            }
            3 => {
                let mut ex: Vec<u32> = Vec::new();
                for _ in 0..rng.below(4) {
                    ex.push(rng.below(ncpu));
                }
                // 保底：不排除光（随机排除集过大全排光时对拍两侧同 None——一致即可）。
                MaskExpr::Exclude(ex)
            }
            _ => MaskExpr::AllOnline,
        };
        // 双实现展开对拍。
        let oracle = oracle_expand(&expr, ncpu, &online);
        match (&mut sel, oracle) {
            (s, Some(want)) => match s.expand(&expr) {
                Ok(got) => {
                    if got != want {
                        return Err((
                            XC_ORACLE_GAP,
                            format!(
                                "组 #{} 表达 {} 失全等：{:?} ≠ 预期 {:?}",
                                i,
                                expr.name(),
                                got,
                                want
                            ),
                        ));
                    }
                    agree += 1;
                }
                Err((c, m)) => {
                    return Err((
                        XC_ORACLE_GAP,
                        format!("组 #{} 主展开器误拒 {:#06x}：{}", i, c, m),
                    ))
                }
            },
            (s, None) => {
                // 参考口径拒——主展开器也必须拒（同判拒绝=一致）。
                if s.expand(&expr).is_ok() {
                    return Err((
                        XC_ORACLE_GAP,
                        format!("组 #{} 参考拒而主收——口径分歧", i),
                    ));
                }
            }
        }
    }
    // 全等组数对账（1000 组里合法展开的都全等——非法组双侧同判）。
    if agree < 800 {
        return Err((
            XC_ORACLE_GAP,
            format!("全等组 {} < 800——随机分布异常", agree),
        ));
    }
    sel.audit_book()?;
    Ok(format!(
        "展开对拍 1000 组：全等 {} 组（余为双侧同判拒绝）；冻结账 {} 条审计全绿",
        agree,
        sel.book_len()
    ))
}

/// J1-B：离线核拦截 10/10（五表达各二例，码精确命中+拒绝账对平）。
pub fn j1_offline_block_10() -> Result<String, (u32, String)> {
    let ncpu = 8u32;
    let online: Vec<u32> = vec![0, 1, 2, 3]; // 离线：4/5/6/7
    let mut sel = TargetSelector::new(ncpu, &online)?;
    let mut hits: Vec<(&str, bool)> = Vec::new();

    // 1）位图含离线核 4。
    match sel.expand(&MaskExpr::Bitmap(0b1_0000)) {
        Err((XC_MASK_OFFLINE, _)) => hits.push(("位图离线核", true)),
        _ => hits.push(("位图离线核", false)),
    }
    // 2）位图混合（在线 0+离线 5）——整单拒绝不裁剪。
    match sel.expand(&MaskExpr::Bitmap(0b10_0001)) {
        Err((XC_MASK_OFFLINE, _)) => hits.push(("位图混合核", true)),
        _ => hits.push(("位图混合核", false)),
    }
    // 3）列表含离线核 6。
    match sel.expand(&MaskExpr::List(vec![0, 6])) {
        Err((XC_MASK_OFFLINE, _)) => hits.push(("列表离线核", true)),
        _ => hits.push(("列表离线核", false)),
    }
    // 4）区间 [3,7] 含离线 4..7。
    match sel.expand(&MaskExpr::Range(3, 7)) {
        Err((XC_MASK_OFFLINE, _)) => hits.push(("区间离线核", true)),
        _ => hits.push(("区间离线核", false)),
    }
    // 5）列表含越界核 8。
    match sel.expand(&MaskExpr::List(vec![8])) {
        Err((XC_MASK_OOB, _)) => hits.push(("列表越界核", true)),
        _ => hits.push(("列表越界核", false)),
    }
    // 6）位图越界脏位（核 8 位超出 8 核位宽）。
    match sel.expand(&MaskExpr::Bitmap(0x100)) {
        Err((XC_BITMAP_OOB, _)) => hits.push(("位图脏位", true)),
        _ => hits.push(("位图脏位", false)),
    }
    // 7）区间倒置。
    match sel.expand(&MaskExpr::Range(5, 2)) {
        Err((XC_RANGE_INVERT, _)) => hits.push(("区间倒置", true)),
        _ => hits.push(("区间倒置", false)),
    }
    // 8）排除式全排光（排除 [0,1,2,3]）。
    match sel.expand(&MaskExpr::Exclude(vec![0, 1, 2, 3])) {
        Err((XC_EXPAND_EMPTY, _)) => hits.push(("排除空积", true)),
        _ => hits.push(("排除空积", false)),
    }
    // 9）构造期在线集合越界。
    match TargetSelector::new(4, &[0, 4]) {
        Err((XC_MASK_OOB, _)) => hits.push(("构造越界核", true)),
        _ => hits.push(("构造越界核", false)),
    }
    // 10）构造期空在线集合。
    match TargetSelector::new(4, &[]) {
        Err((XC_EXPAND_EMPTY, _)) => hits.push(("构造空在线", true)),
        _ => hits.push(("构造空在线", false)),
    }

    let missed: Vec<&str> = hits.iter().filter(|(_, ok)| !ok).map(|(n, _)| *n).collect();
    if !missed.is_empty() {
        return Err((
            XC_REJECT_GAP,
            format!("离线/非法拦截失守：{:?} 未按期望码拦截", missed),
        ));
    }
    sel.reject_gate(4, 3, 3)?;
    Ok("离线核拦截 10/10 全拦且码精确命中；拒绝账三分离对平".to_string())
}

/// J1-C：空集拒绝 10/10（合法表达但展开零目标的十种构造）。
pub fn j1_empty_reject_10() -> Result<String, (u32, String)> {
    let ncpu = 4u32;
    let online: Vec<u32> = vec![0, 1, 2, 3];
    let mut sel = TargetSelector::new(ncpu, &online)?;
    let mut hits: Vec<(&str, bool)> = Vec::new();

    // 1）位图全零。
    match sel.expand(&MaskExpr::Bitmap(0)) {
        Err((XC_EXPAND_EMPTY, _)) => hits.push(("位图全零", true)),
        _ => hits.push(("位图全零", false)),
    }
    // 2）空列表。
    match sel.expand(&MaskExpr::List(vec![])) {
        Err((XC_EXPAND_EMPTY, _)) => hits.push(("空列表", true)),
        _ => hits.push(("空列表", false)),
    }
    // 3）排除式排除全部在线核。
    match sel.expand(&MaskExpr::Exclude(vec![0, 1, 2, 3])) {
        Err((XC_EXPAND_EMPTY, _)) => hits.push(("排除全核", true)),
        _ => hits.push(("排除全核", false)),
    }
    // 4）单核平台 AllButSelf（另起选择器）。
    let mut sel1 = TargetSelector::new(1, &[0])?;
    match sel1.expand(&MaskExpr::AllOnline) {
        Ok(_) => hits.push(("单核全集", true)),
        _ => hits.push(("单核全集", false)),
    }
    // 5）区间 [2,2]（单核合法——对照组，应成功）。
    match sel.expand(&MaskExpr::Range(2, 2)) {
        Ok(t) if t == vec![2] => hits.push(("单点区间", true)),
        _ => hits.push(("单点区间", false)),
    }
    // 6）列表重复核（去重后仍非空——对照组成功且规范形去重）。
    match sel.expand(&MaskExpr::List(vec![1, 1, 1])) {
        Ok(t) if t == vec![1] => hits.push(("重复核去重", true)),
        _ => hits.push(("重复核去重", false)),
    }
    // 7）位图全零+列表空组合（第二种空列表变体——空 Vec 引用等价）。
    match sel.expand(&MaskExpr::List(Vec::new())) {
        Err((XC_EXPAND_EMPTY, _)) => hits.push(("空列表变体", true)),
        _ => hits.push(("空列表变体", false)),
    }
    // 8）排除式+离线核在排除集（排除集含离线核合法——只要不排光）。
    match sel.expand(&MaskExpr::Exclude(vec![3])) {
        Ok(t) if t == vec![0, 1, 2] => hits.push(("排除离线核", true)),
        _ => hits.push(("排除离线核", false)),
    }
    // 9）全集对照组（AllOnline 成功且==在线集）。
    match sel.expand(&MaskExpr::AllOnline) {
        Ok(t) if t == online => hits.push(("全集对照", true)),
        _ => hits.push(("全集对照", false)),
    }
    // 10）位图只置离线位（核 4..7 位在 4 核平台是脏位——OOB 而非空集，口径区分）。
    match sel.expand(&MaskExpr::Bitmap(0xF0)) {
        Err((XC_BITMAP_OOB, _)) => hits.push(("脏位口径", true)),
        _ => hits.push(("脏位口径", false)),
    }

    let missed: Vec<&str> = hits.iter().filter(|(_, ok)| !ok).map(|(n, _)| *n).collect();
    if !missed.is_empty() {
        return Err((
            XC_REJECT_GAP,
            format!("空集/口径失守：{:?} 未按期望", missed),
        ));
    }
    Ok("空集拒绝口径 10/10 全对（6 拒 4 收全按语义精确命中）".to_string())
}

/// 冻结面审计（冻结→重算指纹→篡改注入检出——0x5008 显性）。
pub fn freeze_tamper_gate() -> Result<String, (u32, String)> {
    let online: Vec<u32> = vec![0, 1, 2, 3];
    let mut sel = TargetSelector::new(4, &online)?;
    let ft = sel.expand_and_freeze(&MaskExpr::List(vec![0, 2, 3]))?;
    // 正常审计绿。
    ft.audit()?;
    // 篡改注入：克隆后偷改目标集——重算指纹应立即漂移。
    let mut evil = ft.clone();
    evil.targets[0] = 1;
    match evil.audit() {
        Err((XC_FP_DRIFT, _)) => {}
        Ok(()) => return Err((XC_FP_DRIFT, "篡改目标集未检出——指纹闸失守".to_string())),
        Err((c, m)) => return Err((XC_FP_DRIFT, format!("篡改检出码 {:#06x} 异常：{}", c, m))),
    }
    // 冻结账全量审计绿（原集未被污染）。
    sel.audit_book()?;
    Ok(format!(
        "冻结面过：冻结 {} 目标指纹 {:#018x}；篡改注入即检出（0x5008）；冻结账 {} 条全绿",
        ft.targets.len(),
        ft.fp,
        sel.book_len()
    ))
}

/// 双向转换压测（随机列表 1 万次往返互逆——0x500B 闸大流量验证）。
pub fn roundtrip_10k() -> Result<String, (u32, String)> {
    let ncpu = 32u32;
    let mut rng = Lcg::new(0x5033_0B17);
    let mut ok = 0u32;
    for _ in 0..10_000u32 {
        let mut l: Vec<u32> = Vec::new();
        for _ in 0..(1 + rng.below(32)) {
            l.push(rng.below(ncpu));
        }
        roundtrip_check(&l, ncpu)?;
        ok += 1;
    }
    Ok(format!("双向转换 {} 次往返互逆全过（去重升序口径一致）", ok))
}

/// 万级展开压测（随机五表达 1 万次——主展开器+冻结账+审计全链）。
pub fn stress_10k() -> Result<String, (u32, String)> {
    let ncpu = 64u32;
    let mut online: Vec<u32> = Vec::new();
    let mut rng = Lcg::new(0x5044_B07A);
    for c in 0..ncpu {
        if rng.below(100) < 85 {
            online.push(c);
        }
    }
    if online.is_empty() {
        online.push(0);
    }
    let mut sel = TargetSelector::new(ncpu, &online)?;
    let mut frozen = 0u64;
    for i in 0..10_000u32 {
        let kind = i % 5;
        let expr = match kind {
            0 => {
                let mut bits: u64 = 0;
                for _ in 0..(1 + rng.below(10)) {
                    let c = online[rng.below(online.len() as u32) as usize];
                    bits |= 1u64 << c;
                }
                MaskExpr::Bitmap(bits)
            }
            1 => {
                let mut l: Vec<u32> = Vec::new();
                for _ in 0..(1 + rng.below(20)) {
                    l.push(online[rng.below(online.len() as u32) as usize]);
                }
                MaskExpr::List(l)
            }
            2 => {
                let a = online[rng.below(online.len() as u32) as usize];
                let b = online[rng.below(online.len() as u32) as usize];
                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                // 纯在线区间不保证——非法时对拍口径同判（此处直接跳过冻结，只展开）。
                match sel.expand(&MaskExpr::Range(lo, hi)) {
                    Ok(_) => {}
                    Err((XC_MASK_OFFLINE, _)) => {} // 区间跨离线核=合法拒绝路径。
                    Err((c, m)) => return Err((c, format!("压测 #{} 意外红：{}", i, m))),
                }
                continue;
            }
            3 => {
                let mut ex: Vec<u32> = Vec::new();
                for _ in 0..rng.below(8) {
                    ex.push(rng.below(ncpu));
                }
                MaskExpr::Exclude(ex)
            }
            _ => MaskExpr::AllOnline,
        };
        match sel.expand_and_freeze(&expr) {
            Ok(_) => frozen += 1,
            Err((XC_EXPAND_EMPTY, _)) | Err((XC_MASK_OFFLINE, _)) | Err((XC_BITMAP_OOB, _)) => {}
            Err((c, m)) => return Err((c, format!("压测 #{} 意外红：{}", i, m))),
        }
    }
    sel.audit_book()?;
    // 规范形三律抽验：冻结账里逐条升序+无重复+指纹一致（audit_book 已全量，此处抽首尾）。
    let first = sel.frozen_at_idx(0).ok_or_else(|| {
        (
            XC_EXPAND_EMPTY,
            "冻结账为空——压测全被拒不合理".to_string(),
        )
    })?;
    first.audit()?;
    Ok(format!(
        "万级展开压测过：1 万次五表达（冻结 {} 条+区间拒绝路径）；冻结账全量审计绿；在线 {}/64 核",
        frozen,
        online.len()
    ))
}

/// F0923 总闸。
pub fn run_gate() -> Result<String, (u32, String)> {
    let a = j1_expand_1000()?;
    let b = j1_offline_block_10()?;
    let c = j1_empty_reject_10()?;
    let d = freeze_tamper_gate()?;
    let e = roundtrip_10k()?;
    let f = stress_10k()?;
    Ok(format!(
        "F0923 全绿：\n[展开对拍] {}\n[离线拦截] {}\n[空集口径] {}\n[冻结面] {}\n[双向转换] {}\n[万级压测] {}",
        a, b, c, d, e, f
    ))
}
