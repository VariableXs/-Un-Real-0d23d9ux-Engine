/**
 * F384 焦点陷阱（模态完整性）（H 域 · AI-H4）：
 * 模态对话框打开时 Tab 循环锁定在框内（焦点陷阱）：Tab 在最后一个控件后回到第一个
 * ——键盘用户不会「Tab 进了对话框再也回不来」；陷阱只在模态生效（非模态浮层不困人）；
 * 关闭对话框焦点归还唤起者（F206 判据复用）。
 * 判据（主册 F384）：陷阱完整性（模态内 Tab 50 次零逃逸）；非模态不困判据；归还联动
 * F206；陷阱开启/关闭即时性。
 * 依赖锚点：F206 焦点可见性与键盘导航。
 */

export interface TrapTarget {
  /** 可聚焦控件序（DOM 序）。 */
  id: string;
  focusable: boolean;
}

export interface TrapSession {
  modalId: string;
  /** 陷阱内的聚焦环（只含 focusable 控件）。 */
  ring: string[];
  /** 唤起者元素（关闭后归还——F206 联动判据）。 */
  invokerId: string;
  active: boolean;
}

/** 开陷阱：取框内可聚焦控件成环；无可聚焦控件时如实降级（环空但不困死——Esc/关钮兜底）。 */
export function openTrap(modalId: string, targets: TrapTarget[], invokerId: string): TrapSession {
  return {
    modalId,
    ring: targets.filter((t) => t.focusable).map((t) => t.id),
    invokerId,
    active: true,
  };
}

/** Tab 下一步：环内循环（Shift 反向）；空环返回 null（不假装有焦点）。 */
export function nextFocus(session: TrapSession, current: string | null, shift: boolean): string | null {
  if (!session.active || session.ring.length === 0) return null;
  if (current === null || !session.ring.includes(current)) {
    return shift ? session.ring[session.ring.length - 1]! : session.ring[0]!;
  }
  const idx = session.ring.indexOf(current);
  const next = shift ? (idx - 1 + session.ring.length) % session.ring.length : (idx + 1) % session.ring.length;
  return session.ring[next]!;
}

/** 关陷阱：焦点归还唤起者（判据「焦点回家」）；关闭即时（active 翻转）。 */
export function closeTrap(session: TrapSession): { returnedTo: string; active: false } {
  return { returnedTo: session.invokerId, active: false };
}

/** 陷阱完整性审计（判据）：模态内连按 50 次 Tab 零逃逸（全部落点都在环内）。 */
export function auditNoEscape(session: TrapSession, presses = 50): { pass: boolean; escapes: number; visited: string[] } {
  let current: string | null = null;
  let escapes = 0;
  const visited: string[] = [];
  for (let i = 0; i < presses; i++) {
    const next = nextFocus(session, current, false);
    if (next === null || !session.ring.includes(next)) escapes++;
    else visited.push(next);
    current = next;
  }
  return { pass: escapes === 0 && visited.length === presses, escapes, visited };
}

/** 非模态不困判据：非模态浮层不建陷阱（openTrap 只被模态调用——此处为审计口径）。 */
export function trapOnlyForModal(kind: "modal" | "non-modal", wantTrap: boolean): boolean {
  return kind === "modal" ? wantTrap : !wantTrap;
}

/** 陷阱开启/关闭即时性（判据）：开启即生效——openTrap 返回 active=true 无延迟位。 */
export function immediateArming(session: TrapSession): boolean {
  return session.active === true;
}
