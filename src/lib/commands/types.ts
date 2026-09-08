/**
 * AI-07 效率中枢组 · P-1 命令注册表类型（NEXT-40 P-1 / N-13 先行切片）。
 * 每个用户可执行动作声明为一条 Command；全域统一注册（含后续插件命令经
 * N-26 网关注入）。命令面板 / 统一搜索 / 宏引擎动作库共用这一单一事实源。
 */

export type CommandCategory =
  | "app" // 启动应用 / 打开虚拟窗口应用
  | "action" // 环境动作（贴靠、切换、最小化…）
  | "settings" // 设置项直达
  | "tool" // 内置工具
  | "macro" // 宏引擎动作（N-18 / V-45）
  | "search"; // 搜索入口（N-14）

export interface Command {
  /** 全局唯一 id（点分命名：域.动作，如 "palette.open"、"snap.left"）。 */
  id: string;
  /** 词典 key（cmd*），zh/en 双语（audit 门禁）。 */
  titleKey: string;
  /** 分类（面板内分组 + `?`/`>` 前缀过滤依据）。 */
  category: CommandCategory;
  /** 模糊匹配附加关键词（小写；含中英文与拼音首字母由 registry 统一派生）。 */
  keywords?: string[];
  /** 可见条件（如仅桌面模式）；省略 = 恒可见。 */
  when?: () => boolean;
  /**
   * 执行动作。返回 false = 需要链式参数（面板停留等待后续输入，N-13 链式）。
   */
  action: () => boolean | void | Promise<boolean | void>;
  /** 来源（审计用）：内置模块 id 或插件 id。 */
  source?: string;
  /** 权限标记：插件命令可能因权限被过滤（N-13 验收④）。 */
  pluginId?: string;
}

/** 学习统计（纯本地：频次 + 最近使用时间；不上传）。 */
export interface CommandStat {
  count: number;
  lastUsed: number;
}

/** 钉选（面板顶部常用区，V-45 同款钉选位语义）。 */
export interface PinnedCommands {
  /** 有序 id 列表（拖拽排序）。 */
  ids: string[];
  cap: number;
}

/** 查询结果（带得分，供排序与测试断言）。 */
export interface CommandHit {
  command: Command;
  score: number;
  pinned: boolean;
}
