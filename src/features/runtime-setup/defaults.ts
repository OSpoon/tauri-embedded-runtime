import type { RuntimeKind, SetupPlanStep, WizardStep } from "./types"

export function createDefaultSetupPlan(runtime: RuntimeKind = "python"): SetupPlanStep[] {
  const runtimeStep = runtime === "python"
    ? { id: "python", phase: "python", title: "准备 Python", description: "下载并校验 CPython 运行时" }
    : { id: "node", phase: "node", title: "准备 Node.js", description: "下载并校验便携版 Node.js" }
  const projectStep = runtime === "python"
    ? { id: "project-python", phase: "project-python", title: "安装 FastAPI 环境", description: "创建独立 venv 并安装 Python 项目依赖" }
    : { id: "project-node", phase: "project-node", title: "安装 Express 环境", description: "安装独立 node_modules 并生成 package-lock" }

  return [
    { id: "check", phase: "check", title: "检查安装环境", description: "读取 manifest、锁和平台信息" },
    runtimeStep,
    projectStep,
    { id: "project-tools", phase: "project-tools", title: "检查项目工具", description: "检查 ffmpeg 等项目级工具制品" },
    { id: "verify", phase: "verify", title: "验证运行时", description: "探测解释器并启动服务，检查健康接口" },
  ]
}

export function createDefaultWizardSteps(plan: SetupPlanStep[] = createDefaultSetupPlan()): WizardStep[] {
  return plan.map(step => ({ ...step, status: "pending" }))
}
