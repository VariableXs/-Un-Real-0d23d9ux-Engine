//! AI-03 · 通俗化翻译引擎（#167~#190）。
//!
//! 翻译流程：代码 → AST → 关键词 → 语义ID → 领域适配 → 比喻选取 → 三层校验 → 输出。
//! 零 AI：映射表 + 句式模板 + 确定性三层比喻校验。

/// 一个预设比喻（F184 比喻卡片库条目）。
#[derive(Debug, Clone, Copy)]
pub struct Metaphor {
    pub concept: &'static str,
    /// 所属代码领域（游戏/电商/网络/通用……），禁止跨领域混用。
    pub domain: &'static str,
    /// 比喻的核心动作（第二层校验依据）。
    pub action: &'static str,
    /// 展示文本。
    pub text: &'static str,
}

/// F184 50 个预设比喻（通用 + 游戏领域 + 电商领域）。
pub const METAPHORS: [Metaphor; 50] = [
    Metaphor { concept: "variable", domain: "通用", action: "存放", text: "装东西的盒子" },
    Metaphor { concept: "function", domain: "通用", action: "加工", text: "一台小机器：原料进、成品出" },
    Metaphor { concept: "array", domain: "通用", action: "排列", text: "一排编了号的格子" },
    Metaphor { concept: "loop", domain: "通用", action: "重复", text: "转圈的跑道，跑完一圈又一圈" },
    Metaphor { concept: "if", domain: "通用", action: "分岔", text: "路上的岔道口，看路牌选边" },
    Metaphor { concept: "class", domain: "通用", action: "制造", text: "图纸：照着图纸造出一个个东西" },
    Metaphor { concept: "object", domain: "通用", action: "存在", text: "按图纸造出来的实物" },
    Metaphor { concept: "parameter", domain: "通用", action: "输入", text: "投进机器的原料" },
    Metaphor { concept: "return", domain: "通用", action: "交付", text: "机器吐出来的成品" },
    Metaphor { concept: "boolean", domain: "通用", action: "判断", text: "电灯开关：只有开和关" },
    Metaphor { concept: "string", domain: "通用", action: "串联", text: "一串穿起来的字珠子" },
    Metaphor { concept: "map", domain: "通用", action: "查找", text: "贴着标签的抽屉柜" },
    Metaphor { concept: "queue", domain: "通用", action: "排队", text: "排队买票：先来的先上" },
    Metaphor { concept: "stack", domain: "通用", action: "叠放", text: "一摞盘子：最后放的先拿" },
    Metaphor { concept: "recursion", domain: "通用", action: "嵌套", text: "俄罗斯套娃：打开一层还有一层" },
    Metaphor { concept: "cache", domain: "通用", action: "暂存", text: "手边的便签：常用的先抄下来" },
    Metaphor { concept: "thread", domain: "通用", action: "并行", text: "两个帮厨同时切菜" },
    Metaphor { concept: "lock", domain: "通用", action: "独占", text: "公共厕所的门锁：进去先上锁" },
    Metaphor { concept: "event", domain: "通用", action: "触发", text: "门铃：有人按铃你才去开门" },
    Metaphor { concept: "callback", domain: "通用", action: "回报", text: "留了电话：办好了给你回个话" },
    Metaphor { concept: "exception", domain: "通用", action: "兜底", text: "安全气垫：摔下来也有个接的" },
    Metaphor { concept: "interface", domain: "通用", action: "约定", text: "插座标准：形状对上就能插" },
    Metaphor { concept: "refactor", domain: "通用", action: "整理", text: "大扫除：东西没换，摆放更顺" },
    Metaphor { concept: "bug", domain: "通用", action: "捣乱", text: "混进机器的小虫子" },
    Metaphor { concept: "api", domain: "通用", action: "服务", text: "餐厅点菜窗口：你报菜名它端菜" },
    Metaphor { concept: "player", domain: "游戏", action: "控制", text: "导演手里的大明星" },
    Metaphor { concept: "input", domain: "游戏", action: "接收", text: "遥控器：按一下角色动一下" },
    Metaphor { concept: "game_state", domain: "游戏", action: "记录", text: "棋盘：一眼看清现在战况" },
    Metaphor { concept: "inventory", domain: "游戏", action: "收纳", text: "背包：捡到的装备都塞这里" },
    Metaphor { concept: "npc", domain: "游戏", action: "表演", text: "站在原地发任务的群演" },
    Metaphor { concept: "quest", domain: "游戏", action: "推进", text: "任务卷轴：做完一页翻一页" },
    Metaphor { concept: "level", domain: "游戏", action: "分层", text: "一关一关的地牢" },
    Metaphor { concept: "spawn", domain: "游戏", action: "生成", text: "刷怪点：怪物从这冒出来" },
    Metaphor { concept: "collision", domain: "游戏", action: "碰撞", text: "两团橡皮泥撞在一起" },
    Metaphor { concept: "ai_controller", domain: "游戏", action: "决策", text: "大脑：替角色决定下一步" },
    Metaphor { concept: "score", domain: "游戏", action: "累计", text: "记分牌：进球就画一道" },
    Metaphor { concept: "shop", domain: "电商", action: "售卖", text: "商店：货架上的东西随便挑" },
    Metaphor { concept: "cart", domain: "电商", action: "装载", text: "购物车：看中的先推着走" },
    Metaphor { concept: "order", domain: "电商", action: "成交", text: "小票：买了什么记得清清楚楚" },
    Metaphor { concept: "payment", domain: "电商", action: "结算", text: "收银台：钱货两讫" },
    Metaphor { concept: "warehouse", domain: "电商", action: "储备", text: "仓库：货都睡在这里" },
    Metaphor { concept: "courier", domain: "电商", action: "运送", text: "快递员：把包裹送到门口" },
    Metaphor { concept: "discount", domain: "电商", action: "减免", text: "优惠券：结账时撕掉一角" },
    Metaphor { concept: "review", domain: "电商", action: "评价", text: "留言本：买家们的碎碎念" },
    Metaphor { concept: "search", domain: "电商", action: "筛选", text: "导购员：说出需求它找货" },
    Metaphor { concept: "recommend", domain: "电商", action: "推荐", text: "货架尽头的今日特价" },
    Metaphor { concept: "refund", domain: "电商", action: "退回", text: "退货窗口：钱沿原路回去" },
    Metaphor { concept: "session", domain: "电商", action: "保持", text: "储物柜号码牌：下次来还认得你" },
    Metaphor { concept: "inventory_check", domain: "电商", action: "核对", text: "盘点：仓库里到底还有几件" },
    Metaphor { concept: "logistics", domain: "电商", action: "调度", text: "物流中枢：包裹排队上车" },
];

/// 三层校验结果（匹配度 ≥80 通过）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verdict {
    pub domain_ok: bool,
    pub action_ok: bool,
    pub reverse_score: u32,
}

impl Verdict {
    pub fn passed(&self) -> bool {
        self.score() >= 80
    }
    /// 三层加权总分（40 领域 + 40 动作 + 20 反向，≥80 通过）。
    pub fn score(&self) -> u32 {
        let mut s = 0;
        if self.domain_ok { s += 40; }
        if self.action_ok { s += 40; }
        s + self.reverse_score.min(20)
    }
}

/// 三层校验：领域匹配 → 核心动作一致 → 反向验证 ≥80%。
pub fn validate_metaphor(code_domain: &str, core_action: &str, m: &Metaphor) -> Verdict {
    let domain_ok = m.domain == code_domain || m.domain == "通用";
    let action_ok = action_matches(core_action, m.action);
    // 反向验证：比喻文本 + 动作词与代码核心动作的词汇重合度
    let score = reverse_score(core_action, m);
    Verdict { domain_ok, action_ok, reverse_score: score }
}

fn action_matches(a: &str, b: &str) -> bool {
    let syn: &[(&str, &str)] = &[
        ("存放", "存放"), ("重复", "重复"), ("分岔", "分岔"), ("查找", "查找"),
        ("排队", "排队"), ("独占", "独占"), ("交付", "交付"), ("输入", "输入"),
        ("记录", "记录"), ("控制", "控制"), ("结算", "结算"), ("装载", "装载"),
    ];
    if a == b { return true; }
    syn.iter().any(|(x, y)| (a == *x || a == *y) && (b == *x || b == *y))
}

fn reverse_score(core_action: &str, m: &Metaphor) -> u32 {
    // 反问：拿掉比喻文字，还剩多少与核心动作对应？用字面重合做确定性近似。
    let hit = m.text.contains(core_action) || m.action.contains(core_action) || core_action.contains(m.action);
    if hit { 20 } else if m.action.len() == core_action.len() { 10 } else { 5 }
}

/// F167 技术词 → 日常词（2000+ 映射的种子集，语义ID 匹配非直译）。
pub fn translate_word(w: &str) -> Option<&'static str> {
    Some(match w.to_lowercase().as_str() {
        "function" | "func" | "fn" | "method" => "一段可重复使用的动作",
        "variable" | "var" | "let" | "const" => "一个装东西的盒子",
        "array" | "list" | "vec" => "一排编号的格子",
        "map" | "dict" | "hashmap" => "贴标签的抽屉柜",
        "loop" | "for" | "while" => "重复做一件事",
        "if" | "else" => "看情况选一条路",
        "return" => "交出结果",
        "null" | "nil" | "none" | "undefined" => "空空如也",
        "true" => "是真的",
        "false" => "是假的",
        "class" => "一张造东西的图纸",
        "object" | "instance" => "照图纸造出的实物",
        "string" => "一段文字",
        "int" | "float" | "double" | "number" => "一个数",
        "bool" | "boolean" => "是/否开关",
        "import" | "include" | "require" => "借别人家的工具",
        "export" | "pub" | "public" => "对外营业",
        "private" => "自家私房，外人不能用",
        "try" => "试着做做看",
        "catch" | "except" => "出事了就接住",
        "throw" | "raise" => "把问题抛出去",
        "async" | "await" => "先去干别的，好了再回来",
        "thread" => "一个同时干活的帮手",
        "cache" => "先抄在手边的便签",
        "queue" => "排队的队伍",
        "stack" => "一摞盘子",
        "error" | "exception" => "出了岔子",
        "bug" => "捣乱的小虫子",
        "parse" => "把一串字读成有用的东西",
        "encode" => "翻译成能传的话",
        "decode" => "把传来的话翻译回来",
        "query" => "开口问数据要东西",
        "insert" => "往里塞一条",
        "delete" | "remove" | "del" => "删掉",
        "update" => "更新成新的",
        "select" => "挑出来",
        "sort" => "排个序",
        "filter" => "筛一筛",
        "merge" => "合到一起",
        "split" => "拆开",
        "append" | "push" | "add" => "追加到后面",
        "pop" => "取走最后一个",
        "index" | "idx" => "编号位置",
        "key" => "钥匙/标签",
        "value" => "格子里的东西",
        "token" => "通行令牌",
        "session" => "这次访问的身份证",
        "request" | "req" => "一次请求",
        "response" | "res" | "resp" => "一次答复",
        "server" => "提供服务的那台机器",
        "client" => "上门的客人",
        "socket" => "两台机器之间的电话线",
        "buffer" => "临时周转的托盘",
        "stream" => "细细的水流，一点一点来",
        "config" | "conf" | "cfg" => "说明书（怎么设置）",
        "log" => "值班日记",
        "assert" => "拍胸脯保证",
        "mock" => "替身演员",
        "refactor" => "大扫除",
        "deploy" | "release" => "发货上架",
        "build" | "compile" => "把图纸变成成品",
        "run" | "execute" | "exec" => "开工",
        "init" | "initialize" | "new" => "从头建好",
        "destroy" | "dispose" | "free" => "拆掉回收",
        "open" => "打开",
        "close" => "关上",
        "read" => "读出来",
        "write" => "写进去",
        "count" | "cnt" => "数一数",
        "sum" => "加总",
        "avg" | "average" => "求平均",
        "min" => "最小的",
        "max" => "最大的",
        "len" | "length" | "size" => "有多少个",
        "contains" | "includes" | "has" => "包不包含",
        "empty" | "is_empty" => "是不是空的",
        _ => return None,
    })
}

/// 动作词小词典（F168）。
fn verb_zh(w: &str) -> Option<&'static str> {
    Some(match w {
        "get" | "fetch" | "load" | "read" | "find" | "query" => "获取",
        "set" | "put" | "save" | "write" | "store" => "设置",
        "calc" | "calculate" | "compute" | "sum" => "计算",
        "create" | "make" | "build" | "new" | "init" => "创建",
        "delete" | "remove" | "del" | "clear" => "删除",
        "update" | "modify" | "change" | "edit" => "更新",
        "check" | "valid" | "validate" | "verify" => "检查",
        "parse" | "decode" | "convert" => "解析",
        "send" | "post" | "submit" | "push" => "发送",
        "login" | "signin" | "auth" => "登录",
        "logout" | "signout" => "退出登录",
        "open" => "打开",
        "close" => "关闭",
        "start" | "run" | "launch" | "exec" => "启动",
        "stop" | "cancel" | "abort" | "kill" => "停止",
        "add" | "append" | "insert" => "添加",
        "sort" | "order" => "排序",
        "search" | "filter" => "查找",
        "count" => "统计",
        "draw" | "render" | "paint" | "print" => "绘制",
        "handle" | "on" | "process" => "处理",
        "notify" | "alert" | "warn" => "提醒",
        "move" | "goto" | "navigate" => "移动",
        "connect" => "连接",
        "disconnect" => "断开",
        "download" => "下载",
        "upload" => "上传",
        _ => return None,
    })
}

/// 名词小词典（F169/F170）。
fn noun_zh(w: &str) -> Option<&'static str> {
    Some(match w {
        "user" | "usr" | "customer" | "member" => "用户",
        "cart" | "basket" => "购物车",
        "order" => "订单",
        "item" | "product" | "goods" => "商品",
        "price" | "cost" | "amount" | "amt" | "total" => "金额",
        "name" | "title" => "名称",
        "count" | "cnt" | "qty" | "num" | "quantity" => "数量",
        "date" | "time" => "时间",
        "list" | "array" | "items" => "一列东西",
        "data" | "info" | "record" => "数据",
        "file" | "doc" => "文件",
        "msg" | "message" | "text" => "消息",
        "page" | "view" => "页面",
        "button" | "btn" => "按钮",
        "window" | "win" => "窗口",
        "config" | "conf" | "cfg" | "setting" | "option" => "配置",
        "error" | "err" | "exception" => "错误",
        "result" | "res" | "resp" | "response" | "output" => "结果",
        "status" | "state" => "状态",
        "id" | "uid" | "key" => "编号",
        "index" | "idx" | "pos" => "位置",
        "size" | "len" | "length" => "大小",
        "token" | "ticket" => "令牌",
        "account" | "acct" => "账户",
        "password" | "pwd" | "passwd" => "密码",
        "email" | "mail" => "邮箱",
        "address" | "addr" => "地址",
        "url" | "uri" | "link" => "链接",
        "image" | "img" | "photo" | "pic" => "图片",
        "video" | "vid" => "视频",
        "game" => "游戏",
        "player" => "玩家",
        "level" | "stage" => "关卡",
        "score" | "point" => "得分",
        "manager" | "mgr" => "管家",
        "service" | "svc" => "服务员",
        "handler" => "接待员",
        "store" | "db" | "database" | "repo" | "dao" => "仓库",
        "server" => "服务器",
        "client" => "客户端",
        "cache" => "便签盒",
        "log" => "日记本",
        "task" | "job" => "任务",
        "queue" => "队伍",
        "pool" => "备用池",
        "factory" => "工厂",
        "builder" => "工匠",
        "controller" => "指挥",
        "adapter" | "wrapper" => "转接头",
        "observer" | "listener" => "观察员",
        "strategy" => "锦囊",
        "template" => "模板",
        "context" | "ctx" | "env" => "现场环境",
        "history" | "hist" => "历史记录",
        "version" | "ver" => "版本",
        "test" | "spec" => "考卷",
        _ => return None,
    })
}

/// F170 类名 → 角色名称（职责比喻）。
pub fn class_role(class_name: &str) -> String {
    let toks = crate::nouns::tokenize(class_name);
    let mut out = Vec::new();
    for t in &toks {
        if let Some(z) = noun_zh(t) {
            out.push(z.to_string());
        } else if let Some(z) = verb_zh(t) {
            out.push(z.to_string());
        } else {
            out.push(t.clone());
        }
    }
    if toks.iter().any(|t| t == "manager" || t == "mgr") {
        out.push("（负责管着这一摊）".into());
    } else if toks.iter().any(|t| t == "factory") {
        out.push("（负责批量制造）".into());
    }
    out.join("")
}

/// F168 函数名 → 动作短语（动词在前）。
pub fn fn_phrase(fn_name: &str) -> String {
    let toks = crate::nouns::tokenize(fn_name);
    let mut verbs = Vec::new();
    let mut nouns = Vec::new();
    for t in &toks {
        if let Some(z) = verb_zh(t) {
            verbs.push(z.to_string());
        } else if let Some(z) = noun_zh(t) {
            nouns.push(z.to_string());
        }
    }
    if verbs.is_empty() && nouns.is_empty() {
        return fn_name.to_string();
    }
    verbs.extend(nouns);
    verbs.join("")
}

/// F169 变量名 → 东西名称。
pub fn var_name(var: &str) -> String {
    let toks = crate::nouns::tokenize(var);
    let parts: Vec<String> = toks
        .iter()
        .map(|t| noun_zh(t).map(|s| s.to_string()).unwrap_or_else(|| t.clone()))
        .collect();
    parts.join("")
}

/// F171 参数名 → 输入说明。
pub fn param_desc(name: &str, ty: Option<&str>) -> String {
    let zh = var_name(name);
    fn ty_label(t: &str) -> &'static str {
        match t {
        "int" | "i32" | "i64" | "u32" | "u64" | "f32" | "f64" | "float" | "double" | "number" => "数字",
        "str" | "String" | "string" | "char" => "文字",
        "bool" | "boolean" => "真假",
        other => translate_word(other).unwrap_or("类型"),
        }
    }
    match ty {
        Some(t) => format!("传进去的{}（{}）", zh, ty_label(t)),
        None => format!("传进去的{}", zh),
    }
}

/// F172 返回值 → 输出说明。
pub fn return_desc(expr: &str) -> String {
    format!("交出去：{}", expr)
}

/// 条件表达式 → 中文（F173 用）。
pub fn cond_zh(cond: &str) -> String {
    let c = cond.trim();
    if let Some((l, r)) = c.split_once("==") {
        return format!("{} 等于 {}", var_name(l.trim()), val_zh(r.trim()));
    }
    if let Some((l, r)) = c.split_once("!=") {
        return format!("{} 不等于 {}", var_name(l.trim()), val_zh(r.trim()));
    }
    if let Some((l, r)) = c.split_once(">=") {
        return format!("{} 不小于 {}", var_name(l.trim()), val_zh(r.trim()));
    }
    if let Some((l, r)) = c.split_once("<=") {
        return format!("{} 不大于 {}", var_name(l.trim()), val_zh(r.trim()));
    }
    if let Some((l, r)) = c.split_once('>') {
        return format!("{} 比 {} 大", var_name(l.trim()), val_zh(r.trim()));
    }
    if let Some((l, r)) = c.split_once('<') {
        return format!("{} 比 {} 小", var_name(l.trim()), val_zh(r.trim()));
    }
    if c.contains("&&") {
        let parts: Vec<String> = c.split("&&").map(|p| cond_zh(p)).collect();
        return parts.join("，并且 ");
    }
    if c.contains("||") {
        let parts: Vec<String> = c.split("||").map(|p| cond_zh(p)).collect();
        return parts.join("，或者 ");
    }
    if let Some(stripped) = c.strip_prefix('!') {
        return format!("{} 不成立", var_name(stripped.trim()));
    }
    format!("{} 成立", var_name(c))
}

fn val_zh(v: &str) -> String {
    let v = v.trim().trim_matches(|c| c == '"' || c == '\'');
    if v == "null" || v == "nil" || v == "none" {
        "空".into()
    } else if let Some(z) = noun_zh(v) {
        z.into()
    } else if translate_word(v).is_some() {
        translate_word(v).unwrap().into()
    } else {
        v.into()
    }
}

/// F173 if → 如果…就…
pub fn if_sentence(cond: &str) -> String {
    format!("如果 {}，就…", cond_zh(cond))
}
/// F174 for → 一个一个…
pub fn for_sentence(collection: &str) -> String {
    format!("把 {} 一个一个拿出来，挨个…", var_name(collection.trim()))
}
/// F175 while → 一直…直到
pub fn while_sentence(cond: &str) -> String {
    format!("一直重复，直到「{}」不成立为止", cond_zh(cond))
}
/// F176 try → 试着…万一
pub fn try_sentence(body: &str) -> String {
    format!("试着「{}」，万一出岔子就按预案接住", body.trim())
}
/// F177 return → 交出去
pub fn return_sentence(expr: &str) -> String {
    return_desc(expr)
}
/// F178 赋值 → 放进盒子
pub fn assign_sentence(var: &str, expr: &str) -> String {
    format!("把 {} 放进叫「{}」的盒子里", expr.trim(), var_name(var))
}
/// F179 比较 → 比大小
pub fn compare_sentence(l: &str, r: &str) -> String {
    cond_zh(&format!("{} > {}", l, r))
}
/// F180 逻辑与 → 并且
pub fn and_connector() -> &'static str {
    "并且"
}
/// F181 逻辑或 → 或者
pub fn or_connector() -> &'static str {
    "或者"
}

/// F182 整行翻译：逐词（含驼峰/下划线分词）替换 + 保序拼接。
pub fn translate_line(line: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in line.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            cur.push(ch);
        } else {
            if !cur.is_empty() {
                out.push(word_or_keep(&cur));
                cur.clear();
            }
            let op = op_zh(ch);
            if !op.is_empty() {
                out.push(op.to_string());
            }
        }
    }
    if !cur.is_empty() {
        out.push(word_or_keep(&cur));
    }
    out.join("")
}

fn word_or_keep(w: &str) -> String {
    if let Some(z) = verb_zh(w) {
        return z.into();
    }
    if let Some(z) = noun_zh(w) {
        return z.into();
    }
    if let Some(z) = translate_word(w) {
        return z.into();
    }
    // 驼峰/缩写复合词：分词后逐段翻译
    let toks = crate::nouns::tokenize(w);
    if toks.len() > 1 || (toks.len() == 1 && toks[0] != w) {
        let parts: Vec<String> = toks
            .iter()
            .map(|t| {
                verb_zh(t)
                    .map(|s| s.to_string())
                    .or_else(|| noun_zh(t).map(|s| s.to_string()))
                    .or_else(|| translate_word(t).map(|s| s.to_string()))
                    .or_else(|| crate::nouns::expand_abbrev(t).map(|s| s.to_string()))
                    .unwrap_or_else(|| t.clone())
            })
            .collect();
        return parts.join("");
    }
    w.to_string()
}

fn op_zh(ch: char) -> &'static str {
    match ch {
        '=' => "赋值", '+' => "加", '-' => "减", '*' => "乘", '/' => "除",
        '%' => "取余", '>' => "大于", '<' => "小于", '!' => "非", '&' => "与",
        '|' => "或", '(' => "（", ')' => "）", '{' => "：开始", '}' => "结束",
        ',' => "，", ';' => "；", _ => "",
    }
}

/// F183 整段翻译：逐行 + 连接词润色。
pub fn translate_paragraph(src: &str) -> String {
    let mut parts = Vec::new();
    let mut first_in_block = true;
    for line in src.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let tr = translate_line(t);
        if tr.is_empty() {
            continue;
        }
        if t.starts_with('}') {
            parts.push(format!("到这里结束。{}", tr));
        } else if first_in_block {
            parts.push(format!("先{}", tr));
        } else {
            parts.push(format!("接着{}", tr));
        }
        first_in_block = false;
    }
    parts.join("；")
}

/// F185 数据类型比喻。
pub fn type_metaphor(ty: &str) -> &'static str {
    match ty {
        "int" | "i32" | "i64" | "u32" | "u64" | "float" | "f32" | "f64" | "double" | "number" => "一顆颗数得清的糖",
        "str" | "string" | "String" | "char" => "一串穿好的字珠子",
        "bool" | "boolean" => "开/关 电灯开关",
        "array" | "list" | "Vec" | "Vec<T>" => "一排编号的格子",
        "map" | "dict" | "HashMap" => "贴标签的抽屉柜",
        "set" => "不重样的收藏册",
        "tuple" => "捆在一起的小包裹",
        "Option" | "optional" | "nullable" => "可能空手的快递盒",
        "Result" => "要么有货要么有退款单的快递盒",
        "fn" | "function" | "callback" => "一台随身小机器",
        _ => "一个普通的盒子",
    }
}

/// F186 运算符比喻（中文动词）。
pub fn operator_verb(op: &str) -> &'static str {
    match op {
        "+" => "加上", "-" => "减去", "*" => "乘上", "/" => "除以", "%" => "取余数",
        "=" => "装进", "==" => "看等不等于", "!=" => "看是不是不一样", ">" => "看是否更大",
        "<" => "看是否更小", ">=" => "看是否不小于", "<=" => "看是否不大于",
        "&&" => "并且", "||" => "或者", "!" => "反过来", "++" => "多加一个", "--" => "去掉一个",
        "+=" => "再加上", "-=" => "再减去", _ => "操作",
    }
}

/// F187 错误信息翻译（诊断库）。
pub fn error_plain(err: &str) -> &'static str {
    let e = err.to_lowercase();
    if e.contains("enoent") || e.contains("no such file") {
        "找不到那个文件：路径可能写错了，或者文件被移走了"
    } else if e.contains("eacces") || e.contains("permission denied") {
        "没有权限：系统不让你动这个东西"
    } else if e.contains("econnrefused") || e.contains("connection refused") {
        "对方拒接：服务可能没开，地址可能不对"
    } else if e.contains("timeout") || e.contains("timed out") {
        "等超时了：对方半天没回话"
    } else if e.contains("division by zero") || e.contains("divide by zero") {
        "不能除以零：数学上不成立"
    } else if e.contains("null") || e.contains("undefined") || e.contains("none") {
        "拿到了一个空值：东西可能还没准备好"
    } else if e.contains("out of memory") || e.contains("oom") {
        "内存不够用了：东西太多，盒子装不下"
    } else if e.contains("stack overflow") {
        "套娃套太深了：递归停不下来"
    } else if e.contains("index out of") || e.contains("out of range") || e.contains("out of bounds") {
        "编号越界了：想取第 N 格，但格子没那么多"
    } else if e.contains("syntax") || e.contains("parse error") {
        "句子写错了：编译器读不懂这行"
    } else if e.contains("type") && (e.contains("mismatch") || e.contains("error")) {
        "类型对不上：盒子的形状和东西不配套"
    } else if e.contains("deadlock") {
        "死锁：两个帮手互相等对方放手"
    } else if e.contains("duplicate") || e.contains("already exists") {
        "重复了：同名的东西已经有一个"
    } else if e.contains("not found") || e.contains("404") {
        "没找到：要的东西不存在"
    } else if e.contains("unauthorized") || e.contains("401") {
        "没登录：先亮出通行令牌"
    } else if e.contains("forbidden") || e.contains("403") {
        "禁止入内：登录了也没权限"
    } else if e.contains("econnreset") || e.contains("connection reset") {
        "连接被掐断：对方中途挂了电话"
    } else {
        "出了岔子，但具体原因要看完整报错"
    }
}

/// F188 整文件故事化：声明遍历 + 故事模板。
pub fn file_story(fn_names: &[&str], calls: &[(String, String)]) -> String {
    const CN: [&str; 10] = ["一", "二", "三", "四", "五", "六", "七", "八", "九", "十"];
    let mut story = String::from("这个故事讲的是：");
    for (i, f) in fn_names.iter().enumerate() {
        if i > 0 {
            story.push_str("；");
        }
        let cn = CN.get(i).copied().unwrap_or("?");
        story.push_str(&format!("第{}章「{}」", cn, fn_phrase(f)));
    }
    story.push_str("。\n情节走向：");
    let mut edges = 0;
    for (from, to) in calls {
        story.push_str(&format!("{} 完成后喊来 {}；", fn_phrase(from), fn_phrase(to)));
        edges += 1;
    }
    if edges == 0 {
        story.push_str("各章独立，没有互相喊话。");
    }
    story
}

/// F189 逐行中文旁注。
pub fn annotate_lines(src: &str) -> Vec<(usize, String)> {
    src.lines()
        .enumerate()
        .map(|(i, l)| (i + 1, translate_line(l)))
        .filter(|(_, t)| !t.is_empty())
        .collect()
}

/// F190 语法糖展开翻译：先脱糖再翻。
pub fn desugar(line: &str) -> String {
    let t = line.trim();
    if let Some((l, r)) = t.split_once("?.") {
        return format!("如果 {} 存在，取 {}; 否则跳过", l.trim(), r.trim());
    }
    if let Some((l, r)) = t.split_once("??") {
        return format!("如果 {} 为空就用 {}，否则用 {}", l.trim(), r.trim(), l.trim());
    }
    if t.contains(" ? ") && t.contains(" : ") {
        if let Some((c, rest)) = t.split_once(" ? ") {
            if let Some((a, b)) = rest.split_once(" : ") {
                return format!("如果 {} 就是 {}，否则是 {}", c.trim(), a.trim(), b.trim());
            }
        }
    }
    if let Some((l, r)) = t.split_once("+=") {
        return format!("把 {} 再加上 {}", var_name(l.trim()), r.trim());
    }
    if let Some((l, r)) = t.split_once("-=") {
        return format!("把 {} 减去 {}", var_name(l.trim()), r.trim());
    }
    if t.ends_with("++") {
        return format!("把 {} 多加一个", var_name(t.trim_end_matches("++")));
    }
    if t.contains("...") || t.contains("..") {
        if let Some((a, b)) = t.split_once("..") {
            return format!("从 {} 一路数到 {}（不含尽头）", a.trim().trim_end_matches('.'), b.trim());
        }
    }
    if let Some((l, r)) = t.split_once("=>") {
        return format!("给它 {}，它就替你 {}", l.trim(), r.trim());
    }
    translate_line(t)
}

pub fn run_translate_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("translate");
    // F167
    s.add("#167 技术词→日常词", translate_word("variable") == Some("一个装东西的盒子") && translate_word("async").is_some(), "语义ID匹配非直译");
    // F168
    let p = fn_phrase("calcTotal");
    s.add("#168 函数名→动作短语", p == "计算金额", "分词+动词+名词词典");
    // F169
    s.add("#169 变量名→东西名称", var_name("userCart") == "用户购物车", "变量节点中文标签");
    // F170
    let cr = class_role("OrderManager");
    s.add("#170 类名→角色名称", cr.contains("订单") && cr.contains("管家"), "角色词典+职责说明");
    // F171
    s.add("#171 参数名→输入说明", param_desc("userId", Some("int")).contains("用户") && param_desc("userId", Some("int")).contains("数字"), "参数旁说明气泡");
    // F172
    s.add("#172 返回值→输出说明", return_desc("total").starts_with("交出去"), "返回箭头旁说明");
    // F173
    s.add("#173 if→如果…就…", if_sentence("age > 18") == "如果 age 比 18 大，就…", "判断菱形中文条件");
    // F174
    s.add("#174 for→一个一个…", for_sentence("items").contains("一个一个"), "循环六边形中文");
    // F175
    s.add("#175 while→一直…直到", while_sentence("done == false").contains("一直重复"), "循环持续条件");
    // F176
    s.add("#176 try-catch→试着…万一", try_sentence("保存文件").contains("试着") && try_sentence("保存文件").contains("万一"), "异常节点意外句式");
    // F177
    s.add("#177 return→交出去", return_sentence("result").contains("交出去"), "交付句式");
    // F178
    s.add("#178 赋值→放进盒子", assign_sentence("cart", "new Cart()").contains("放进"), "容器比喻");
    // F179
    s.add("#179 比较→比大小", compare_sentence("a", "b").contains("比"), "比较节点中文");
    // F180
    s.add("#180 逻辑与→并且", and_connector() == "并且", "连接词");
    // F181
    s.add("#181 逻辑或→或者", or_connector() == "或者", "选择词");
    // F182
    let tl = translate_line("calcTotal(user)");
    s.add("#182 整行翻译", tl.contains("计算") && tl.contains("用户"), "行右侧灰色翻译");
    // F183
    let tp = translate_paragraph("let a = 1\nreturn a\n");
    s.add("#183 整段翻译", tp.contains("先") && tp.contains("接着"), "逐行+连接词润色");
    // F184（含三层校验全链路）
    let game_m = METAPHORS.iter().find(|m| m.concept == "inventory").unwrap();
    let ok = validate_metaphor("游戏", "收纳", game_m);
    let bad = validate_metaphor("电商", "排序", game_m);
    s.add("#184 概念比喻卡片", METAPHORS.len() == 50 && ok.passed() && !(bad.domain_ok && bad.action_ok), "50预设比喻+三层校验（领域/动作/反向≥80%）");
    // F185
    s.add("#185 数据类型比喻", type_metaphor("array").contains("格子") && type_metaphor("bool").contains("开关"), "类型旁日常例子");
    // F186
    s.add("#186 运算符比喻", operator_verb("+") == "加上" && operator_verb("==").contains("等于"), "运算符中文动词");
    // F187
    s.add("#187 错误信息翻译", error_plain("ENOENT: no such file").contains("找不到") && error_plain("division by zero").contains("除以零"), "错误码→大白话诊断");
    // F188
    let story = file_story(&["calcTotal", "sendOrder"], &[("calcTotal".into(), "sendOrder".into())]);
    s.add("#188 整文件故事化", story.contains("第一章") && story.contains("情节走向"), "声明遍历+故事模板");
    // F189
    let ann = annotate_lines("let a = 1\n\nreturn a\n");
    s.add("#189 逐行中文旁注", ann.len() == 2 && ann[0].0 == 1, "行号+灰色小字");
    // F190
    s.add("#190 语法糖展开翻译", desugar("a?.b").contains("如果 a 存在") && desugar("x ?? y").contains("为空就用") && desugar("c ? p : q").contains("否则是"), "脱糖→翻译");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f184_all_50_metaphors_unique_concept_domain() {
        assert_eq!(METAPHORS.len(), 50);
        for m in &METAPHORS {
            assert!(!m.text.is_empty());
            assert!(m.domain == "通用" || m.domain == "游戏" || m.domain == "电商");
        }
        // 同域内概念不重复
        let mut game: Vec<&str> = METAPHORS.iter().filter(|m| m.domain == "游戏").map(|m| m.concept).collect();
        game.sort();
        let n = game.len();
        game.dedup();
        assert_eq!(game.len(), n);
    }

    #[test]
    fn three_layer_validation() {
        // 领域禁止跨用：游戏比喻在电商领域 + 动作不符 → 不通过
        let inv = METAPHORS.iter().find(|m| m.concept == "inventory").unwrap();
        assert!(validate_metaphor("游戏", "收纳", inv).passed());
        assert!(!validate_metaphor("电商", "排序", inv).passed());
        // 通用比喻任何领域都过第一层，但动作要一致
        let var = METAPHORS.iter().find(|m| m.concept == "variable").unwrap();
        assert!(validate_metaphor("电商", "存放", var).passed());
        assert!(!validate_metaphor("电商", "排序", var).passed());
    }

    #[test]
    fn sentence_templates() {
        assert_eq!(if_sentence("x == 1"), "如果 x 等于 1，就…");
        assert_eq!(cond_zh("a && b"), cond_zh("a") + "，并且 " + &cond_zh("b"));
        assert!(while_sentence("true").starts_with("一直重复"));
    }

    #[test]
    fn line_translation_is_deterministic() {
        let a = translate_line("getUser(userId)");
        let b = translate_line("getUser(userId)");
        assert_eq!(a, b);
        assert!(a.contains("获取") || a.contains("用户"));
    }

    #[test]
    fn error_catalog_hits() {
        for e in ["ENOENT", "EACCES", "timeout", "stack overflow", "index out of bounds"] {
            assert_ne!(error_plain(e), "出了岔子，但具体原因要看完整报错", "{e} 应有专属翻译");
        }
    }

    #[test]
    fn desugar_all_forms() {
        assert!(desugar("u?.name").contains("存在"));
        assert!(desugar("a ?? b").contains("否则用"));
        assert!(desugar("n += 1").contains("再加上"));
        assert!(desugar("i++").contains("多加一个"));
        assert!(desugar("0..10").contains("一路数到"));
    }
}
