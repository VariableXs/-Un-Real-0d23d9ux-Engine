//! vcs05_isolate — CGPU-F2885 用户数据隔离（数据/配置/缓存三面 · W02 复用声明）。
//!
//! 判据原文（施工书锚点 CGPU-F2885）：隔离：用户数据隔离（数据/配置/缓存
//! 按用户隔离——隔离实现（W02 数据隔离范式复用——复用声明；测试（隔离/
//! 复用两组。判据：三隔离、复用声明、两组、判据。
//!
//! # 复用声明（判据"W02 数据隔离范式复用——复用声明"的锚定正文）
//!
//! 本模块**复用 W02 数据隔离范式**（CGPU-F3522 文档信息架构确立的命名空
//! 间隔离纪律），四条范式逐条兑现、不另立：
//! 一、**命名空间隔离**——数据按 (用户, 面, 键) 三元组寻址，用户维度在
//!     桶不在键里（键注入跨空间不可能——"u2:xx" 只是普通键名不解析）；
//! 二、**默认拒绝**——查无即查无（[`IsoStore::get`] 跨用户/跨面/不存在
//!     统一 None，不区分"没有"与"不可见"，**存在性不泄露**——错误信息
//!     也是信息，泄露存在性就是泄露半条数据）；
//! 三、**写前登记**——面未开通即拒写（[`E_IS_NOFACE`]），登记闭集显性
//!     化（三面 Data/Config/Cache 各自独立开通互不搭车）；
//! 四、**账面唯一**——读写走同一账（[`IsoStore`]），无影子副本（查询
//!     走账不走缓存副本——副本会漂移，漂移即隔离破口）。
//! 三面差异只在容量语义：Data/Config 无限账、Cache 有界 FIFO 淘汰
//! （缓存本来就该让位——最旧先走，确定性可对拍）。
//! 零 panic（无 unwrap/expect）；诊断码段 0x54（字符串码）。
use alloc::format;
use alloc::string::String;

use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const ISOLATE_VERSION: &str = "S05-isolate-v1";

/// 缓存面容量（FIFO 淘汰线——每用户每面独立容量）。
pub const CACHE_CAP: usize = 8;

/// 诊断码（字符串码四码闭集）。
pub const E_IS_NOFACE: &str = "E_IS_NOFACE";
pub const E_IS_NOKEY: &str = "E_IS_NOKEY";
pub const E_IS_FROZEN: &str = "E_IS_FROZEN";
pub const E_IS_NOARG: &str = "E_IS_NOARG";

/// 诊断详情（四码闭集逐码中文说明）。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_IS_NOFACE => "隔离面未开通：写前登记——三面各自开通互不搭车",
        E_IS_NOKEY => "键不存在或不可见：统一 None——存在性不泄露",
        E_IS_FROZEN => "配置面已冻结：只读锁定生效——冻结后拒写显性不静默",
        E_IS_NOARG => "键名为空：空键不可入账——空键是注入的种子",
        _ => "未知隔离诊断码",
    }
}

/// 隔离面三闭集（判据"数据/配置/缓存按用户隔离"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IsoFace {
    /// 数据面（用户内容数据——无限账）。
    Data,
    /// 配置面（用户偏好设置——可冻结只读）。
    Config,
    /// 缓存面（有界 FIFO——最旧先走）。
    Cache,
}

impl IsoFace {
    pub const ALL: [IsoFace; 3] = [IsoFace::Data, IsoFace::Config, IsoFace::Cache];

    pub const fn zh(self) -> &'static str {
        match self {
            IsoFace::Data => "数据",
            IsoFace::Config => "配置",
            IsoFace::Cache => "缓存",
        }
    }

    pub const fn ordinal(self) -> usize {
        match self {
            IsoFace::Data => 0,
            IsoFace::Config => 1,
            IsoFace::Cache => 2,
        }
    }
}

/// 单条目（三元组寻址的值行）。
#[derive(Clone, Debug)]
pub struct IsoEntry {
    pub user: u32,
    pub face: IsoFace,
    pub key: alloc::string::String,
    pub value: u64,
}

/// 隔离账（判据"三隔离"本体：三面 × 多用户矩阵 + 缓存 FIFO）。
pub struct IsoStore {
    /// 开通面集（写前登记——(user, face) 闭集）。
    faces: Vec<(u32, IsoFace)>,
    /// 冻结面集（Config 只读锁——(user, face)）。
    frozen: Vec<(u32, IsoFace)>,
    /// 条目账（唯一权威——无影子副本）。
    entries: Vec<IsoEntry>,
}

impl IsoStore {
    pub fn new() -> Self {
        IsoStore { faces: Vec::new(), frozen: Vec::new(), entries: Vec::new() }
    }

    /// 开通面（写前登记；重复开通幂等——集合语义）。
    pub fn open_face(&mut self, user: u32, face: IsoFace) {
        let mut found = false;
        for f in &self.faces {
            if f.0 == user && f.1 == face {
                found = true;
            }
        }
        if !found {
            self.faces.push((user, face));
        }
    }

    /// 冻结面（Config 只读锁——冻结后拒写显性；重复冻结幂等）。
    pub fn freeze(&mut self, user: u32, face: IsoFace) {
        let mut found = false;
        for f in &self.frozen {
            if f.0 == user && f.1 == face {
                found = true;
            }
        }
        if !found {
            self.frozen.push((user, face));
        }
    }

    /// 面是否已冻结。
    pub fn is_frozen(&self, user: u32, face: IsoFace) -> bool {
        for f in &self.frozen {
            if f.0 == user && f.1 == face {
                return true;
            }
        }
        false
    }

    /// 写（判据"隔离实现"写侧：面登记闸 → 冻结闸 → 空键闸 → 入账；
    /// 缓存面满载 FIFO 逐最旧——淘汰计数可对账）。
    pub fn put(&mut self, user: u32, face: IsoFace, key: &str, value: u64) -> Result<(), &'static str> {
        if key.is_empty() {
            return Err(E_IS_NOARG);
        }
        let mut opened = false;
        for f in &self.faces {
            if f.0 == user && f.1 == face {
                opened = true;
            }
        }
        if !opened {
            return Err(E_IS_NOFACE);
        }
        if self.is_frozen(user, face) {
            return Err(E_IS_FROZEN);
        }
        // 覆盖写（同键更新——更新不是新增）。
        for e in &mut self.entries {
            if e.user == user && e.face == face && e.key == key {
                e.value = value;
                return Ok(());
            }
        }
        // 缓存面 FIFO 淘汰（满载逐最旧——同用户同面的最早条目）。
        if face == IsoFace::Cache {
            let mut count = 0usize;
            for e in &self.entries {
                if e.user == user && e.face == face {
                    count += 1;
                }
            }
            while count >= CACHE_CAP {
                let mut victim = None;
                for (i, e) in self.entries.iter().enumerate() {
                    if e.user == user && e.face == face {
                        victim = Some(i);
                        break;
                    }
                }
                match victim {
                    Some(i) => {
                        self.entries.remove(i);
                        count -= 1;
                    }
                    None => break,
                }
            }
        }
        self.entries.push(IsoEntry {
            user,
            face,
            key: alloc::format!("{}", key),
            value,
        });
        Ok(())
    }

    /// 读（判据"隔离"读侧 + W02 默认拒绝范式：跨用户/跨面/不存在统一
    /// None——存在性不泄露，调用方无从区分三种"没有"）。
    pub fn get(&self, user: u32, face: IsoFace, key: &str) -> Option<u64> {
        for e in &self.entries {
            if e.user == user && e.face == face && e.key == key {
                return Some(e.value);
            }
        }
        None
    }

    /// 删（删除也是写——冻结闸同拒；返回是否存在）。
    pub fn del(&mut self, user: u32, face: IsoFace, key: &str) -> Result<bool, &'static str> {
        if self.is_frozen(user, face) {
            return Err(E_IS_FROZEN);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].user == user
                && self.entries[i].face == face
                && self.entries[i].key == key
            {
                self.entries.remove(i);
                return Ok(true);
            }
            i += 1;
        }
        Ok(false)
    }

    /// 某用户某面条目数（对账口）。
    pub fn count_of(&self, user: u32, face: IsoFace) -> usize {
        let mut n = 0usize;
        for e in &self.entries {
            if e.user == user && e.face == face {
                n += 1;
            }
        }
        n
    }

    /// 账面总条目。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "隔离{} 面{} 条目{}",
            ISOLATE_VERSION,
            self.faces.len(),
            self.entries.len(),
        )
    }
}

/// 判据主入口（注册于 cgpu/mod.rs blocks 数组，tag CGPU-F2885）。
pub fn run_vcs05_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-f2885");

    // --- 隔离组（判据一）---
    // 隔-01：三面隔离互不串——u1 三面各写同名键 "cfg" 不同值，三面各自
    // 读回自己的值（面维度隔离）；三维闭集 ordinal 0..=2 连续中文名互异。
    let mut st = IsoStore::new();
    st.open_face(1, IsoFace::Data);
    st.open_face(1, IsoFace::Config);
    st.open_face(1, IsoFace::Cache);
    let _ = st.put(1, IsoFace::Data, "cfg", 100);
    let _ = st.put(1, IsoFace::Config, "cfg", 200);
    let _ = st.put(1, IsoFace::Cache, "cfg", 300);
    let face_iso = st.get(1, IsoFace::Data, "cfg") == Some(100)
        && st.get(1, IsoFace::Config, "cfg") == Some(200)
        && st.get(1, IsoFace::Cache, "cfg") == Some(300);
    let faces = IsoFace::ALL;
    let kind_ok = faces.len() == 3
        && faces[0].ordinal() == 0
        && faces[1].ordinal() == 1
        && faces[2].ordinal() == 2
        && faces[0].zh() != faces[1].zh()
        && faces[0].zh() != faces[2].zh()
        && faces[1].zh() != faces[2].zh();
    if face_iso && kind_ok {
        s.ok("F2885-隔-01-三面隔离互不串");
    } else {
        s.fail("F2885-隔-01-三面隔离互不串", "面隔离失效");
    }

    // --- 隔离组（判据二）---
    // 隔-02：跨用户不可见 + 存在性不泄露——u1 写 "secret" 后，u2 读同名
    // 键 None（u2 未开通面/未写键两种"没有"统一 None 无从区分）；
    // u2 开通面后读仍 None（键空间独立——不是面问题）；
    // 键注入面：键名 "u1:secret" 只是普通键名不解析路由（用户维度在桶
    // 不在键里——W02 命名空间范式）。
    let mut st2 = IsoStore::new();
    st2.open_face(1, IsoFace::Data);
    st2.open_face(2, IsoFace::Data);
    let _ = st2.put(1, IsoFace::Data, "secret", 777);
    let blind = st2.get(2, IsoFace::Data, "secret").is_none();
    let blind2 = st2.get(2, IsoFace::Config, "secret").is_none();
    let _ = st2.put(2, IsoFace::Data, "u1:secret", 888);
    let inject_ok = st2.get(2, IsoFace::Data, "u1:secret") == Some(888)
        && st2.get(1, IsoFace::Data, "u1:secret").is_none()
        && st2.get(2, IsoFace::Data, "secret").is_none();
    if blind && blind2 && inject_ok {
        s.ok("F2885-隔-02-跨用户不可见存在性不泄露");
    } else {
        s.fail("F2885-隔-02-跨用户不可见存在性不泄露", "用户隔离失效");
    }

    // --- 隔离组（判据三）---
    // 隔-03：写前登记闸 + 空键闸 + 冻结闸——未开面拒 E_IS_NOFACE；
    // 空键拒 E_IS_NOARG；冻结后拒写 E_IS_FROZEN 但读照常（只读语义）。
    let mut st3 = IsoStore::new();
    let noface = st3.put(5, IsoFace::Data, "k", 1);
    st3.open_face(5, IsoFace::Config);
    let noarg = st3.put(5, IsoFace::Config, "", 1);
    let _ = st3.put(5, IsoFace::Config, "theme", 1);
    st3.freeze(5, IsoFace::Config);
    let frozen = st3.put(5, IsoFace::Config, "theme", 2);
    let read_ok = st3.get(5, IsoFace::Config, "theme") == Some(1);
    let gates_ok = matches!(noface, Err(E_IS_NOFACE))
        && matches!(noarg, Err(E_IS_NOARG))
        && matches!(frozen, Err(E_IS_FROZEN))
        && read_ok
        && st3.is_frozen(5, IsoFace::Config);
    if gates_ok {
        s.ok("F2885-隔-03-登记空键冻结三闸");
    } else {
        s.fail("F2885-隔-03-登记空键冻结三闸", "写闸失效");
    }

    // --- 隔离组（判据四）---
    // 隔-04：缓存 FIFO 淘汰恰值——容量 8：写 9 条后首条被逐（count 恰 8
    // 且首键不在次键在）；第 10 条写入逐第 2 条；删除也是写闸（冻结拒删）；
    // 覆盖写不新增（同键更新 count 不变）。
    let mut st4 = IsoStore::new();
    st4.open_face(6, IsoFace::Cache);
    let mut i = 0u32;
    while i < 9 {
        let key = alloc::format!("k{}", i);
        let _ = st4.put(6, IsoFace::Cache, &key, i as u64);
        i += 1;
    }
    let cap_ok = st4.count_of(6, IsoFace::Cache) == CACHE_CAP
        && st4.get(6, IsoFace::Cache, "k0").is_none()
        && st4.get(6, IsoFace::Cache, "k1").is_some();
    let _ = st4.put(6, IsoFace::Cache, "k9", 9);
    let fifo2 = st4.count_of(6, IsoFace::Cache) == CACHE_CAP
        && st4.get(6, IsoFace::Cache, "k1").is_none()
        && st4.get(6, IsoFace::Cache, "k2").is_some();
    let _ = st4.put(6, IsoFace::Cache, "k8", 999);
    let overwrite = st4.count_of(6, IsoFace::Cache) == CACHE_CAP
        && st4.get(6, IsoFace::Cache, "k8") == Some(999);
    if cap_ok && fifo2 && overwrite {
        s.ok("F2885-隔-04-缓存FIFO恰值淘汰");
    } else {
        s.fail("F2885-隔-04-缓存FIFO恰值淘汰", "FIFO 失效");
    }

    // --- 复用组（判据五）---
    // 复-01：W02 数据隔离范式复用声明锚——四范式兑现对拍：命名空间隔离
    // （键注入不路由——判据二已证）/默认拒绝（三种"没有"统一 None）/
    // 写前登记（E_IS_NOFACE）/账面唯一（len==逐面 count 之和——无影子
    // 副本自洽闭合）+ 四码互异文案互异 + 版本锚。
    let mut st5 = IsoStore::new();
    st5.open_face(1, IsoFace::Data);
    st5.open_face(1, IsoFace::Cache);
    let _ = st5.put(1, IsoFace::Data, "a", 1);
    let _ = st5.put(1, IsoFace::Data, "b", 2);
    let _ = st5.put(1, IsoFace::Cache, "a", 3);
    let sum_faces = st5.count_of(1, IsoFace::Data) + st5.count_of(1, IsoFace::Cache);
    let sole_ok = st5.len() == 3 && sum_faces == 3;
    let codes = [E_IS_NOFACE, E_IS_NOKEY, E_IS_FROZEN, E_IS_NOARG];
    let mut uniq = true;
    for i in 0..4 {
        for j in (i + 1)..4 {
            if codes[i] == codes[j] {
                uniq = false;
            }
        }
    }
    let mut det_uniq = true;
    for i in 0..4 {
        for j in (i + 1)..4 {
            if reason_of(codes[i]) == reason_of(codes[j]) {
                det_uniq = false;
            }
        }
    }
    let unknown_ok = reason_of("E_UNKNOWN") == "未知隔离诊断码";
    let ver_ok = ISOLATE_VERSION == "S05-isolate-v1" && CACHE_CAP == 8;
    if sole_ok && uniq && det_uniq && unknown_ok && ver_ok {
        s.ok("F2885-复-01-W02范式复用声明锚");
    } else {
        s.fail("F2885-复-01-W02范式复用声明锚", "复用声明失效");
    }

    // --- 复用组（判据六）---
    // 复-02：删除语义——del 存在键真删（再读 None）+ 不存在键 Ok(false)
    // 不报错（删除不存在不是事故）+ 冻结面拒删 E_IS_FROZEN（删除也是写）；
    // 空账全查询平凡零 panic。
    let mut st6 = IsoStore::new();
    st6.open_face(1, IsoFace::Data);
    let _ = st6.put(1, IsoFace::Data, "gone", 5);
    let d1 = st6.del(1, IsoFace::Data, "gone");
    let d1_ok = matches!(d1, Ok(true)) && st6.get(1, IsoFace::Data, "gone").is_none();
    let d2 = st6.del(1, IsoFace::Data, "never");
    let d2_ok = matches!(d2, Ok(false));
    st6.freeze(1, IsoFace::Data);
    let d3 = st6.del(1, IsoFace::Data, "x");
    let d3_ok = matches!(d3, Err(E_IS_FROZEN));
    let empty = IsoStore::new();
    let empty_ok = empty.is_empty()
        && empty.len() == 0
        && empty.get(1, IsoFace::Data, "x").is_none()
        && empty.summary().contains("条目0");
    if d1_ok && d2_ok && d3_ok && empty_ok {
        s.ok("F2885-复-02-删除语义与空态平凡");
    } else {
        s.fail("F2885-复-02-删除语义与空态平凡", "删除语义失效");
    }

    s
}
