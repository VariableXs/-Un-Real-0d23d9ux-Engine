/**
 * 批次E-8（规格 N5）：拼音 / 首字母搜索。
 * 内置常用字表（覆盖 UI 词汇与常见应用名词）；未收录汉字按原样保留，
 * 不猜音 —— 如实降级为子串匹配。只做只读匹配，零网络。
 */

/** 常用字 → 全拼（不带声调）。多音字取 UI 语境最高频读音。 */
const PINYIN: Record<string, string> = {
  安: "an", 案: "an", 按: "an", 阿: "a", 啊: "a", 爱: "ai", 矮: "ai",
  北: "bei", 本: "ben", 比: "bi", 笔: "bi", 边: "bian", 变: "bian", 标: "biao",
  表: "biao", 别: "bie", 不: "bu", 部: "bu", 步: "bu", 版: "ban", 办: "ban",
  半: "ban", 包: "bao", 报: "bao", 备: "bei", 被: "bei", 背: "bei", 奔: "ben",
  编: "bian", 便: "bian", 辨: "bian", 宾: "bin", 并: "bing", 病: "bing", 波: "bo",
  白: "bai", 百: "bai", 摆: "bai", 败: "bai", 搬: "ban", 板: "ban", 保: "bao",
  帮: "bang", 棒: "bang", 杯: "bei", 悲: "bei", 逼: "bi", 鼻: "bi", 笔记: "biji",
  批: "pi", 片: "pian", 篇: "pian", 品: "pin", 平: "ping", 屏: "ping", 破: "po",
  铺: "pu", 普: "pu", 皮: "pi", 匹: "pi", 偏: "pian", 凭: "ping",
  次: "ci", 词: "ci", 此: "ci", 从: "cong", 存: "cun", 错: "cuo", 才: "cai",
  菜: "cai", 参: "can", 残: "can", 藏: "cang", 操: "cao", 草: "cao", 测: "ce",
  层: "ceng", 差: "cha", 查: "cha", 产: "chan", 常: "chang", 场: "chang", 唱: "chang",
  超: "chao", 车: "che", 成: "cheng", 城: "cheng", 程: "cheng", 吃: "chi", 持: "chi",
  重: "chong", 抽: "chou", 出: "chu", 除: "chu", 处: "chu", 传: "chuan", 窗: "chuang",
  床: "chuang", 创: "chuang", 吹: "chui", 垂: "chui", 纯: "chun", 说: "shuo",
  达: "da", 打: "da", 大: "da", 代: "dai", 带: "dai", 待: "dai", 单: "dan",
  但: "dan", 当: "dang", 刀: "dao", 到: "dao", 道: "dao", 得: "de", 的: "de",
  等: "deng", 低: "di", 地: "di", 底: "di", 点: "dian", 电: "dian", 掉: "diao",
  订: "ding", 定: "ding", 丢: "diu", 东: "dong", 动: "dong", 冻: "dong", 都: "dou",
  斗: "dou", 读: "du", 度: "du", 端: "duan", 短: "duan", 段: "duan", 断: "duan",
  队: "dui", 对: "dui", 多: "duo", 复: "fu", 法: "fa", 发: "fa", 反: "fan",
  饭: "fan", 方: "fang", 放: "fang", 房: "fang", 访: "fang", 飞: "fei", 分: "fen",
  份: "fen", 丰: "feng", 风: "feng", 封: "feng", 否: "fou", 服: "fu", 浮: "fu",
  符: "fu", 幅: "fu", 夫: "fu", 附: "fu", 副: "fu", 赋: "fu", 付: "fu",
  个: "ge", 各: "ge", 给: "gei", 跟: "gen", 更: "geng", 工: "gong", 公: "gong",
  功: "gong", 共: "gong", 狗: "gou", 够: "gou", 古: "gu", 故: "gu", 关: "guan",
  观: "guan", 管: "guan", 广: "guang", 归: "gui", 规: "gui", 果: "guo", 过: "guo",
  国: "guo", 高: "gao", 告: "gao", 格: "ge", 隔: "ge", 感: "gan", 敢: "gan",
  刚: "gang", 钢: "gang", 改: "gai", 盖: "gai", 干: "gan", 挂: "gua", 怪: "guai",
  和: "he", 合: "he", 后: "hou", 候: "hou", 号: "hao", 好: "hao", 黑: "hei",
  很: "hen", 红: "hong", 宏: "hong", 换: "huan", 环: "huan", 还: "hai", 海: "hai",
  害: "hai", 含: "han", 汉: "han", 航: "hang", 号码: "haoma", 毫: "hao",
  河: "he", 荷: "he", 核: "he", 贺: "he", 横: "heng", 衡: "heng", 互: "hu",
  户: "hu", 护: "hu", 花: "hua", 化: "hua", 话: "hua", 怀: "huai", 坏: "huai",
  欢: "huan", 回: "hui", 会: "hui", 绘: "hui", 婚: "hun", 混: "hun", 活: "huo",
  火: "huo", 获: "huo", 或: "huo", 货: "huo", 忽: "hu", 湖: "hu", 乎: "hu",
  级: "ji", 机: "ji", 及: "ji", 即: "ji", 极: "ji", 集: "ji", 计: "ji",
  记: "ji", 纪: "ji", 技: "ji", 际: "ji", 剂: "ji", 家: "jia", 加: "jia",
  价: "jia", 假: "jia", 尖: "jian", 间: "jian", 件: "jian", 建: "jian", 健: "jian",
  见: "jian", 键: "jian", 江: "jiang", 将: "jiang", 讲: "jiang", 交: "jiao", 教: "jiao",
  角: "jiao", 脚: "jiao", 叫: "jiao", 接: "jie", 节: "jie", 结: "jie", 结束: "jieshu",
  解: "jie", 界: "jie", 金: "jin", 今: "jin", 仅: "jin", 进: "jin", 近: "jin",
  精: "jing", 经: "jing", 静: "jing", 境: "jing", 九: "jiu", 久: "jiu", 旧: "jiu",
  局: "ju", 句: "ju", 拒: "ju", 具: "ju", 据: "ju", 卷: "juan", 决: "jue",
  绝: "jue", 军: "jun", 均: "jun", 几: "ji", 己: "ji", 计算器: "jisuanqi", 加载: "jiazai",
  开: "kai", 看: "kan", 康: "kang", 考: "kao", 科: "ke", 可: "ke", 刻: "ke",
  客: "ke", 课: "ke", 空: "kong", 口: "kou", 库: "ku", 块: "kuai", 快: "kuai",
  宽: "kuan", 况: "kuang", 困: "kun", 扩: "kuo", 卡: "ka", 开发: "kaifa",
  拉: "la", 来: "lai", 蓝: "lan", 老: "lao", 乐: "le", 类: "lei", 累: "lei",
  冷: "leng", 离: "li", 里: "li", 理: "li", 力: "li", 立: "li", 联: "lian",
  连: "lian", 脸: "lian", 良: "liang", 两: "liang", 亮: "liang", 列: "lie", 林: "lin",
  临: "lin", 零: "ling", 灵: "ling", 另: "ling", 流: "liu", 六: "liu", 楼: "lou",
  路: "lu", 录: "lu", 乱: "luan", 略: "lue", 论: "lun", 落: "luo", 罗: "luo",
  马: "ma", 码: "ma", 妈: "ma", 满: "man", 慢: "man", 忙: "mang", 猫: "mao",
  毛: "mao", 没: "mei", 每: "mei", 美: "mei", 门: "men", 们: "men", 米: "mi",
  密: "mi", 面: "mian", 民: "min", 明: "ming", 名: "ming", 命: "ming", 模: "mo",
  末: "mo", 目: "mu", 木: "mu", 幕: "mu", 媒: "mei", 买: "mai",
  迈: "mai", 卖: "mai", 梦: "meng", 迷: "mi", 秒: "miao", 描: "miao",
  那: "na", 南: "nan", 脑: "nao", 内: "nei", 能: "neng", 你: "ni", 年: "nian",
  念: "nian", 鸟: "niao", 您: "nin", 牛: "niu", 农: "nong", 弄: "nong", 女: "nv",
  暖: "nuan", 难: "nan", 哪: "na", 拿: "na",
  欧: "ou", 偶: "ou",
  牌: "pai", 排: "pai", 盘: "pan", 判: "pan", 旁: "pang", 跑: "pao", 配: "pei",
  朋: "peng", 票: "piao", 拼音: "pinyin",
  期: "qi", 其: "qi", 奇: "qi", 齐: "qi", 起: "qi", 气: "qi", 器: "qi",
  前: "qian", 钱: "qian", 强: "qiang", 墙: "qiang", 抢: "qiang", 桥: "qiao",
  且: "qie", 亲: "qin", 清: "qing", 情: "qing", 请: "qing", 轻: "qing", 秋: "qiu",
  求: "qiu", 区: "qu", 曲: "qu", 去: "qu", 全: "quan", 权: "quan", 缺: "que",
  确: "que", 群: "qun", 千: "qian", 签: "qian", 浅: "qian", 欠: "qian", 切: "qie",
  然: "ran", 让: "rang", 热: "re", 人: "ren", 认: "ren", 任: "ren", 日: "ri",
  容: "rong", 如: "ru", 入: "ru", 软: "ruan", 若: "ruo", 弱: "ruo",
  赛: "sai", 三: "san", 色: "se", 杀: "sha", 山: "shan", 商: "shang", 上: "shang",
  少: "shao", 设: "she", 社: "she", 深: "shen", 什: "shen", 身: "shen", 神: "shen",
  生: "sheng", 声: "sheng", 胜: "sheng", 失: "shi", 十: "shi", 时: "shi", 识: "shi",
  实: "shi", 使: "shi", 始: "shi", 式: "shi", 示: "shi", 世: "shi", 事: "shi",
  是: "shi", 适: "shi", 收: "shou", 手: "shou", 首: "shou", 数: "shu", 属: "shu",
  术: "shu", 树: "shu", 双: "shuang", 水: "shui", 顺: "shun", 四: "si", 似: "si",
  私: "si", 思: "si", 送: "song", 搜: "sou", 速: "su", 素: "su", 算: "suan",
  随: "sui", 岁: "sui", 所: "suo", 锁: "suo", 删: "shan", 筛: "shai", 师: "shi",
  试: "shi", 视: "shi", 输: "shu", 书: "shu", 熟: "shu", 竖: "shu",
  太: "tai", 台: "tai", 谈: "tan", 探: "tan", 汤: "tang", 堂: "tang", 套: "tao",
  特: "te", 提: "ti", 题: "ti", 体: "ti", 天: "tian", 填: "tian", 条: "tiao",
  调: "tiao", 贴: "tie", 铁: "tie", 听: "ting", 停: "ting", 通: "tong", 同: "tong",
  头: "tou", 图: "tu", 途: "tu", 团: "tuan", 推: "tui", 退: "tui", 吞: "tun",
  托: "tuo", 脱: "tuo", 椭: "tuo", 添: "tian", 跳: "tiao", 听写: "tingxie",
  外: "wai", 完: "wan", 玩: "wan", 晚: "wan", 万: "wan", 网: "wang", 往: "wang",
  忘: "wang", 为: "wei", 位: "wei", 卫: "wei", 未: "wei", 文: "wen", 闻: "wen",
  稳: "wen", 问: "wen", 我: "wo", 无: "wu", 五: "wu", 物: "wu", 误: "wu",
  务: "wu", 微: "wei", 维: "wei", 委: "wei", 温: "wen", 卧: "wo", 握: "wo",
  西: "xi", 析: "xi", 习: "xi", 系: "xi", 细: "xi", 下: "xia", 先: "xian",
  现: "xian", 线: "xian", 相: "xiang", 香: "xiang", 想: "xiang", 向: "xiang", 项: "xiang",
  消: "xiao", 小: "xiao", 效: "xiao", 些: "xie", 写: "xie", 谢: "xie", 心: "xin",
  新: "xin", 信: "xin", 行: "xing", 形: "xing", 型: "xing", 性: "xing", 修: "xiu",
  需: "xu", 许: "xu", 序: "xu", 续: "xu", 选: "xuan", 学: "xue", 雪: "xue",
  寻: "xun", 训: "xun", 讯: "xun", 喜: "xi", 洗: "xi", 戏: "xi", 显: "xian",
  限: "xian", 献: "xian", 乡: "xiang", 详: "xiang", 响: "xiang", 像: "xiang", 协: "xie",
  携: "xie", 兴: "xing", 星: "xing", 幸: "xing", 休: "xiu", 宣: "xuan", 悬: "xuan",
  压: "ya", 呀: "ya", 严: "yan", 言: "yan", 颜: "yan", 眼: "yan", 演: "yan",
  阳: "yang", 样: "yang", 要: "yao", 也: "ye", 页: "ye", 夜: "ye", 一: "yi",
  医: "yi", 依: "yi", 移: "yi", 遗: "yi", 已: "yi", 以: "yi", 义: "yi",
  议: "yi", 易: "yi", 因: "yin", 音: "yin", 引: "yin", 隐: "yin", 应: "ying",
  英: "ying", 影: "ying", 迎: "ying", 用: "yong", 优: "you", 由: "you", 游: "you",
  友: "you", 有: "you", 又: "you", 右: "you", 于: "yu", 余: "yu", 鱼: "yu",
  娱: "yu", 与: "yu", 宇: "yu", 语: "yu", 预: "yu", 元: "yuan", 员: "yuan",
  原: "yuan", 圆: "yuan", 远: "yuan", 院: "yuan", 愿: "yuan", 约: "yue", 月: "yue",
  越: "yue", 云: "yun", 允: "yun", 运: "yun", 息: "xi", 吸: "xi", 虾: "xia", 现实: "xianshi", 象: "xiang", 削: "xiao", 笑: "xiao", 鞋: "xie",
  沿: "yan", 宴: "yan", 焰: "yan", 雁: "yan", 央: "yang", 杨: "yang", 洋: "yang",
  仰: "yang", 养: "yang", 氧: "yang", 邀: "yao", 药: "yao", 爷: "ye", 业: "ye",
  叶: "ye", 仪: "yi", 疑: "yi", 乙: "yi", 艺: "yi", 译: "yi", 意: "yi",
  溢: "yi", 银: "yin", 印: "yin", 硬: "ying", 拥: "yong", 永: "yong",
  勇: "yong", 涌: "yong", 忧: "you", 邮: "you", 铀: "you", 愉: "yu", 遇: "yu",
  御: "yu", 裕: "yu", 冤: "yuan", 渊: "yuan", 援: "yuan", 缘: "yuan", 源: "yuan",
  钥: "yue", 悦: "yue", 跃: "yue", 韵: "yun", 匝: "za", 杂: "za", 灾: "zai",
  载: "zai", 再: "zai", 在: "zai", 咱: "zan", 赞: "zan", 脏: "zang", 葬: "zang",
  早: "zao", 造: "zao", 责: "ze", 择: "ze", 增: "zeng", 扎: "zha", 摘: "zhai",
  窄: "zhai", 站: "zhan", 张: "zhang", 章: "zhang", 涨: "zhang", 掌: "zhang", 丈: "zhang",
  招: "zhao", 找: "zhao", 照: "zhao", 罩: "zhao", 折: "zhe", 这: "zhe", 针: "zhen",
  侦: "zhen", 真: "zhen", 镇: "zhen", 阵: "zhen", 挣: "zheng", 争: "zheng", 整: "zheng",
  正: "zheng", 证: "zheng", 政: "zheng", 之: "zhi", 支: "zhi", 只: "zhi", 指: "zhi",
  制: "zhi", 治: "zhi", 质: "zhi", 致: "zhi", 智: "zhi", 中: "zhong", 终: "zhong",
  种: "zhong", 周: "zhou", 州: "zhou", 昼: "zhou", 主: "zhu", 住: "zhu",
  助: "zhu", 注: "zhu", 贮: "zhu", 抓: "zhua", 专: "zhuan", 转: "zhuan", 赚: "zhuan",
  桌: "zhuo", 姿: "zi", 资: "zi", 子: "zi", 自: "zi", 字: "zi", 走: "zou",
  奏: "zou", 租: "zu", 足: "zu", 组: "zu", 阻: "zu", 祖: "zu", 钻: "zuan",
  最: "zui", 尊: "zun", 昨: "zuo", 左: "zuo", 作: "zuo", 坐: "zuo", 座: "zuo",
  做: "zuo", 展: "zhan", 战: "zhan", 帐: "zhang", 账: "zhang", 障: "zhang", 兆: "zhao",
  哲: "zhe", 蔗: "zhe", 枕: "zhen", 峥: "zheng", 帧: "zheng", 症: "zheng",
  芝: "zhi", 值: "zhi", 职: "zhi", 址: "zhi", 纸: "zhi", 挚: "zhi", 掷: "zhi",
  钟: "zhong", 肿: "zhong", 众: "zhong", 皱: "zhou", 逐: "zhu", 烛: "zhu", 嘱: "zhu",
  铸: "zhu", 筑: "zhu", 爪: "zhua", 状: "zhuang", 追: "zhui", 准: "zhun", 捉: "zhuo",
  琢: "zhuo", 啄: "zhuo", 综: "zong", 总: "zong", 纵: "zong",
  嘴: "zui", 醉: "zui", 遵: "zun",
};

/** 取字符串的拼音索引串：汉字转全拼、拉丁/数字原样小写。 */
export function pinyinOf(s: string): string {
  let out = "";
  for (const ch of s) {
    const py = PINYIN[ch];
    if (py) out += py;
    else if (/[a-z0-9]/i.test(ch)) out += ch.toLowerCase();
    // 未收录汉字与空白/符号跳过
  }
  return out;
}

/** 取字符串的首字母索引串：汉字取拼音首字母、拉丁取小写首字符。 */
export function initialsOf(s: string): string {
  let out = "";
  for (const ch of s) {
    const py = PINYIN[ch];
    if (py) out += py[0] ?? "";
    else if (/[a-z0-9]/i.test(ch)) out += ch.toLowerCase();
  }
  return out;
}

/**
 * 拼音匹配（规格 N5）：
 * - 原文子串命中（不区分大小写）
 * - 全拼包含查询（如 "siweidaotu" 命中 "思维导图"）
 * - 首字母命中（如 "sz" 命中 "设置"，"swdt" 命中 "思维导图"）
 * 查询为空 = 不匹配（调用方自行处理空态）。
 */
export function matchPinyin(text: string, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return false;
  if (text.toLowerCase().includes(q)) return true;
  const py = pinyinOf(text);
  if (py && py.includes(q)) return true;
  const ini = initialsOf(text);
  return ini.length > 0 && ini.includes(q);
}

/** 判断查询是否可能是拼音（纯拉丁字母），供调用方决定是否启用拼音回退。 */
export function isAsciiQuery(q: string): boolean {
  return /^[a-z]+$/i.test(q.trim());
}
