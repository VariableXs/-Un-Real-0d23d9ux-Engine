/**
 * 设置中心「个性化」页（E 域 PersonaStudio 内嵌形态）——
 * 设置分类页 + Studio overlay 双入口可达（十二查·双入口通则）。
 */
import { PersonaStudio } from "../../system/persona/PersonaStudio";

export function PersonaTab(): React.ReactNode {
  return <PersonaStudio embedded />;
}
