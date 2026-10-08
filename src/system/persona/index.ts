/**
 * E 个性化域 barrel：PersonaStudio（双形态）+ 底座总线。
 * 挂载路径①：bus.ts "ai04:open-feature" feature="persona-studio"（懒加载默认导出）；
 * 挂载路径②：设置中心 PersonaTab（embedded 形态）。
 */
export { PersonaStudio } from "./PersonaStudio";
export { default } from "./PersonaStudio";
export { personaStore, PersonaStoreError, PERSONA_SECTIONS } from "./store";
