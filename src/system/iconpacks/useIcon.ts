/**
 * N-12 消费接线 hook（iconpacks 领地内提供给桌面/开始菜单车道使用）。
 *
 * 接线点文档（这两个文件归其它车道，集成阶段接一行即可）：
 * - src/system/desktop-icons/DesktopIcons.tsx：渲染系统图标处
 *   `const icon = useIcon("desktop:sys-recycle")`（键 = 区域:名称，回收站空/满
 *   两态即两个键 desktop:sys-recycle / desktop:sys-recycle-full）；
 *   icon 非空时直接渲染资源（svg 用 dangerouslySetInnerHTML + scrubSvg 已净化，
 *   dataURL 用 <img src>），为 null 走原有原生提取路径（IShellItemImageFactory）。
 * - src/system/startmenu/StartMenu.tsx：应用入口处
 *   `const icon = useIcon("start:app-" + appMode)`；第三方软件可用
 *   "start:third:<id>"。任务栏/文件管理器同理（taskbar:* / file:<ext>）。
 * 回退纪律：useIcon 未命中返回 null（消费方回退原生），并计入 getFallbackCount()
 * 会话级缺键回退计数（N-12 验收口径），绝不出现空洞。
 */
import { useMemo } from "react";
import { getIconOverride, useIconPackVersion } from "./registry";
import { iconResource } from "./vicon";

export function useIcon(key: string): { kind: "svg" | "img"; value: string } | null {
  // 版本号变化（安装/卸载）触发重查；memo 内调用 getIconOverride，
  // 缺键回退计数每次 (key, packVersion) 只累计一次（useMemo 语义）。
  const version = useIconPackVersion();
  return useMemo(() => {
    const hit = getIconOverride(key);
    return hit ? iconResource(hit) : null;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, version]);
}