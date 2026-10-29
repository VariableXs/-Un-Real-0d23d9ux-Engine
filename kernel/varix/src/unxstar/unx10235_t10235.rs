//! UNX-F10235 失绿知识库（典型模式入册可检索）（0x114A0 段）
//!
//! 锚点：UNX-F10235-J1 失绿模式入册（≥8 类典型模式登记且指纹唯一）；
//! J2 可检索（按症状关键词/指纹/类别三路检索命中 10/10）；J3 重复入册
//! 拒收与冻结红线（重指纹拒收、冻结后写拒收 10/10 次检出）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·归因周期收口组）。
//!
//! 结构：模式册（症状签名 × 根因类别 × 处方）+ 三路检索核（关键词/
//! 指纹/类别）+ 命中率账 + 注入核（重指纹/空症状/冻结后写必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x114A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x114A0;
pub const KB_DUP_FP: u32 = SEG_BASE + 0x01; // 重指纹
pub const KB_EMPTY_SYM: u32 = SEG_BASE + 0x02; // 空症状
pub const KB_BAD_CAT: u32 = SEG_BASE + 0x03; // 错类别
pub const KB_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后写
pub const KB_NO_RX: u32 = SEG_BASE + 0x05; // 无处方

/// 根因类别闭集（B40 收口谱系）。
pub const C_REGRESS: u8 = 0; // 回归引入
pub const C_RACE: u8 = 1; // 竞态窗口
pub const C_CAP: u8 = 2; // 容量越界
pub const C_DRIFT: u8 = 3; // 数字漂移
pub const C_UNDECL: u8 = 4; // 未声明依赖
pub const C_ORDER: u8 = 5; // 注册顺序
pub const C_SPEC: u8 = 6; // 判据自相矛盾
pub const C_ENV: u8 = 7; // 标准环境错
pub const CAT_N: u8 = 8;

/// 失绿模式条目。
#[derive(Clone, PartialEq)]
pub struct Pattern {
    pub id: u32,
    pub fp: u32,
    pub cat: u8,
    pub symptom: String,
    pub rx: String,
}

impl Pattern {
    pub fn ok(&self) -> bool {
        self.fp != 0
            && self.cat < CAT_N
            && !self.symptom.is_empty()
            && !self.rx.is_empty()
    }

    pub fn render(&self) -> String {
        format!("P{}|fp{:08x}|c{}|{}", self.id, self.fp, self.cat, self.symptom)
    }
}

/// FNV-1a 32 位指纹：症状文本折叠。
pub fn sym_fp(symptom: &str) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for b in symptom.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// 失绿知识库册。
pub struct KbBook {
    rows: Vec<Pattern>,
    fps: Vec<u32>,
    frozen: bool,
    hits: Vec<(u32, u8)>, // (模式id, 检索路) 0=关键词 1=指纹 2=类别
}

impl KbBook {
    pub fn new() -> KbBook {
        KbBook {
            rows: Vec::new(),
            fps: Vec::new(),
            frozen: false,
            hits: Vec::new(),
        }
    }

    /// 入册：重指纹/空症状/错类别/无处方/冻结后写全拒。
    pub fn add(
        &mut self,
        id: u32,
        cat: u8,
        symptom: &str,
        rx: &str,
    ) -> Result<u32, &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if symptom.is_empty() {
            return Err("empty_symptom");
        }
        if cat >= CAT_N {
            return Err("bad_cat");
        }
        if rx.is_empty() {
            return Err("no_rx");
        }
        let fp = sym_fp(symptom);
        if self.fps.contains(&fp) {
            return Err("dup_fp");
        }
        self.fps.push(fp);
        self.rows.push(Pattern {
            id,
            fp,
            cat,
            symptom: symptom.to_string(),
            rx: rx.to_string(),
        });
        Ok(fp)
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn all_ok(&self) -> bool {
        self.rows.iter().all(|p| p.ok())
    }

    /// 路 1：症状关键词检索（子串命中）。
    pub fn by_keyword(&mut self, kw: &str) -> Vec<u32> {
        let mut out = Vec::new();
        for p in &self.rows {
            if p.symptom.contains(kw) {
                out.push(p.id);
                self.hits.push((p.id, 0));
            }
        }
        out
    }

    /// 路 2：指纹精确检索。
    pub fn by_fp(&mut self, fp: u32) -> Option<u32> {
        for p in &self.rows {
            if p.fp == fp {
                self.hits.push((p.id, 1));
                return Some(p.id);
            }
        }
        None
    }

    /// 路 3：类别检索。
    pub fn by_cat(&mut self, cat: u8) -> Vec<u32> {
        let mut out = Vec::new();
        for p in &self.rows {
            if p.cat == cat {
                out.push(p.id);
                self.hits.push((p.id, 2));
            }
        }
        out
    }

    /// 命中率账：三路总命中数。
    pub fn hit_count(&self) -> usize {
        self.hits.len()
    }

    /// 处方获取：按 id 取处方文本。
    pub fn rx_of(&self, id: u32) -> Option<&str> {
        self.rows.iter().find(|p| p.id == id).map(|p| p.rx.as_str())
    }
}

/// 注入核：向册注入指定故障模式。
pub fn inject_fault(b: &mut KbBook, code: u32, id: u32) -> Result<u32, &'static str> {
    match code {
        KB_DUP_FP => {
            let _ = b.add(id, C_RACE, "锁序倒置挂死", "恢复锁序");
            b.add(id + 1, C_CAP, "锁序倒置挂死", "改用一致序")
        }
        KB_EMPTY_SYM => b.add(id, C_RACE, "", "处方一"),
        KB_BAD_CAT => b.add(id, 200, "症状", "处方"),
        KB_FROZEN => {
            b.freeze();
            b.add(id, C_ENV, "冻结后症状", "冻结后处方")
        }
        KB_NO_RX => b.add(id, C_ENV, "无处方症状", ""),
        _ => Err("unknown"),
    }
}

/// 命中率评测：已知模式集 × 查询集，三路各应命中。
pub fn hit_audit(b: &mut KbBook, probe: &str, fp: u32, cat: u8) -> (usize, usize) {
    let mut hit = 0;
    let total = 3;
    if !b.by_keyword(probe).is_empty() {
        hit += 1;
    }
    if b.by_fp(fp).is_some() {
        hit += 1;
    }
    if !b.by_cat(cat).is_empty() {
        hit += 1;
    }
    (hit, total)
}

/// 预置八类典型失绿模式（判据基线）。
pub fn seed(b: &mut KbBook) -> Result<(), &'static str> {
    let seeds: [(u32, u8, &str, &str); 8] = [
        (1, C_REGRESS, "改判据后整册恒红", "把判据改对而非降低强度"),
        (2, C_RACE, "并行覆写丢 #[repr(u16)]", "批量补回并重跑编译"),
        (3, C_CAP, "样本量超册容量报 E_CAP", "先核依赖模块容量再写判据"),
        (4, C_SPEC, "标准环境自相矛盾整册未跑", "先确认夹具真的能构造成功"),
        (5, C_ORDER, "注册行函数名写长名 E0425", "注册行从模块 grep 复制短名"),
        (6, C_UNDECL, "缺 use alloc::vec 三件套", "no_std 模块进链前自查导入"),
        (7, C_ENV, "探针绿真 crate 红", "探针 std prelude 掩盖缺导入"),
        (8, C_ORDER, "段位 off-by-one 跨段撞槽", "领段前跑全段冲突扫描"),
    ];
    for (id, cat, sym, rx) in seeds {
        b.add(id, cat, sym, rx)?;
    }
    Ok(())
}

/// 批内判据。
pub fn run_unx10235_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10235");

    // J1 八类模式入册且指纹唯一、条目全合法
    let (j1_n, j1_fp, j1_ok) = {
        let mut b = KbBook::new();
        let _ = seed(&mut b);
        let mut uniq = true;
        for i in 0..b.rows.len() {
            for j2 in (i + 1)..b.rows.len() {
                if b.rows[i].fp == b.rows[j2].fp {
                    uniq = false;
                }
            }
        }
        (b.len(), uniq, b.all_ok())
    };
    s.add("J1-八类入册", j1_n == 8 && j1_fp && j1_ok, "≥8 类指纹唯一全合法");

    // J2a 关键词检索命中（判据/容量等关键词）
    let j2a = {
        let mut b = KbBook::new();
        let _ = seed(&mut b);
        let r1 = b.by_keyword("容量");
        let r2 = b.by_keyword("E0425");
        r1.len() >= 1 && r2.len() >= 1 && b.hit_count() >= 2
    };
    s.add("J2a-关键词三命中", j2a, "按症状词命中处方可取");

    // J2b 指纹检索精确命中
    let j2b = {
        let mut b = KbBook::new();
        let fp = b.add(9, C_RACE, "双写互踩指纹漂移", "单写者协议");
        match fp {
            Ok(f) => b.by_fp(f) == Some(9),
            Err(_) => false,
        }
    };
    s.add("J2b-指纹精确命中", j2b, "指纹检索路 1/1");

    // J2c 类别检索命中且处方可取
    let j2c = {
        let mut b = KbBook::new();
        let _ = seed(&mut b);
        let ids = b.by_cat(C_ORDER);
        ids.len() == 2 && b.rx_of(ids[0]).is_some()
    };
    s.add("J2c-类别检索", j2c, "类别路命中且处方非空");

    // J3 注入面五路：重指纹/空症状/错类别/冻结后写/无处方
    let codes = [KB_DUP_FP, KB_EMPTY_SYM, KB_BAD_CAT, KB_FROZEN, KB_NO_RX];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut KbBook::new(), **c, 99).is_err())
        .count();
    s.add("J3-注入五路", caught == 5, "五路注入全检出");

    // J4 重指纹拒因精确
    let j4 = inject_fault(&mut KbBook::new(), KB_DUP_FP, 50) == Err("dup_fp");
    s.add("J4-重指纹拒因", j4, "dup_fp 精确拒收");

    // J5 冻结后写拒收
    let j5 = {
        let mut b = KbBook::new();
        inject_fault(&mut b, KB_FROZEN, 60);
        b.is_frozen()
            && b.add(61, C_ENV, "冻结后新症状", "处方") == Err("frozen")
    };
    s.add("J5-冻结拒收", j5, "印后登记必抓");

    // J6 命中率评测：预置册三路全中（3/3）
    let j6 = {
        let mut b = KbBook::new();
        let _ = seed(&mut b);
        let fp = sym_fp("并行覆写丢 #[repr(u16)]");
        let (h, t) = hit_audit(&mut b, "覆写", fp, C_RACE);
        h == t && t == 3
    };
    s.add("J6-三路全中", j6, "3/3 检索路命中");

    // J7 负样本：坏类别/坏条目 ok() 必假
    let j7 = {
        let bad = Pattern {
            id: 1,
            fp: 0,
            cat: C_RACE,
            symptom: "s".to_string(),
            rx: "r".to_string(),
        };
        !bad.ok()
    };
    s.add("J7-零指纹负样本", j7, "零指纹 ok 必假");

    // J8 十次重复入册回放：同一症状二次入册 10/10 拒收
    let j8 = {
        let mut ok = 0;
        for r in 0..10u32 {
            let mut b = KbBook::new();
            let _ = b.add(1, C_CAP, "容量红", "扩容");
            if b.add(2, C_CAP, "容量红", "扩容") == Err("dup_fp") {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J8-十轮重指纹", j8, "10/10 次拒收");

    // J9 处方文本可达：入册→检索→处方闭环
    let j9 = {
        let mut b = KbBook::new();
        let _ = seed(&mut b);
        let ids = b.by_keyword("三件套");
        !ids.is_empty()
            && b.rx_of(ids[0])
                .map(|r| r.contains("导入"))
                .unwrap_or(false)
    };
    s.add("J9-处方闭环", j9, "检索到处方文本闭环");

    // J10 渲染行含指纹十六进制
    let j10 = {
        let mut b = KbBook::new();
        let fp = b.add(3, C_ENV, "渲染指纹检查", "看行").unwrap();
        let row = b.rows.iter().find(|p| p.id == 3).unwrap();
        row.render().contains(&format!("{:08x}", fp))
    };
    s.add("J10-渲染含指纹", j10, "渲染行带指纹十六进制");

    s
}
