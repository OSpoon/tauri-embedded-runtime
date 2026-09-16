import type { UnlistenFn } from "@tauri-apps/api/event"
import type {
  ProjectInfo,
  ProjectSnapshot,
  RuntimeEvent,
  RuntimeKind,
  RuntimeSnapshot,
  ServiceName,
  ServiceSnapshot,
  SetupPlanStep,
  WizardStep,
} from "./types"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { computed, onMounted, onUnmounted, ref } from "vue"
import { createDefaultSetupPlan, createDefaultWizardSteps } from "./defaults"

const isTauri = "__TAURI_INTERNALS__" in window

function initialSnapshot(runtime: RuntimeKind = "python"): RuntimeSnapshot {
  const requirements = { python: runtime === "python", node: runtime === "node" }
  return {
    status: "checking",
    platform: "web",
    arch: "browser",
    runtime_root: "",
    active_generation: null,
    runtime_revision: "runtime-policy-1",
    requirements,
    selected_runtime: runtime,
    python: { required: requirements.python, present: false, status: requirements.python ? "missing" : "disabled", path: null, version: null, issues: [] },
    node: { required: requirements.node, present: false, status: requirements.node ? "missing" : "disabled", path: null, version: null, issues: [] },
    projects: [],
    message: "正在准备运行时检测…",
    checked_at: 0,
    operation_id: null,
    operation_status: "idle",
    issues: [],
  }
}

export function useRuntimeSetup() {
  const runtimeSnapshot = ref<RuntimeSnapshot>(initialSnapshot())
  const selectedRuntime = ref<RuntimeKind | null>(null)
  const runtimeProgress = ref(0)
  const runtimeEvents = ref<RuntimeEvent[]>([])
  const services = ref<ServiceSnapshot[]>([])
  const projectCatalog = ref<ProjectInfo[]>([])
  const runtimeActionRunning = ref(false)
  const setupStarted = ref(false)
  const demoMessage = ref("完成检测并启动服务后，可以从这里调用本地服务。")
  const setupPlan = ref<SetupPlanStep[]>(createDefaultSetupPlan())
  const wizardSteps = ref<WizardStep[]>(createDefaultWizardSteps(setupPlan.value))
  let stopRuntimeEvents: UnlistenFn | null = null

  const runtimeStatusLabel = computed(() => {
    switch (runtimeSnapshot.value.status) {
      case "ready": return "Ready"
      case "missing": return "Needs setup"
      case "corrupted": return "Needs repair"
      case "outdated": return "Update available"
      case "failed": return "Check failed"
      default: return "Checking"
    }
  })

  const runtimeDotClass = computed(() => {
    if (runtimeSnapshot.value.status === "ready")
      return "bg-emerald-500"
    if (runtimeSnapshot.value.status === "checking")
      return "bg-amber-400"
    return "bg-red-500"
  })

  const runtimePathLabel = computed(() => runtimeSnapshot.value.runtime_root || "App data / runtime")

  function stepIdForEvent(event: RuntimeEvent) {
    if (event.phase === "cancel" || event.phase === "cancelled") {
      return wizardSteps.value.find(step => step.status === "active")?.id ?? "check"
    }
    if (wizardSteps.value.some(step => step.id === event.phase))
      return event.phase
    if (event.phase === "download" || event.phase === "extract") {
      return wizardSteps.value.find(step => step.status === "active")?.id ?? "check"
    }
    if (event.phase === "services" || event.phase === "complete") {
      return wizardSteps.value.find(step => step.id === "verify")?.id
        ?? wizardSteps.value[wizardSteps.value.length - 1]?.id
        ?? "check"
    }
    if (event.phase === "preflight")
      return "check"
    if (event.phase === "install") {
      return wizardSteps.value.find(step => step.status === "active")?.id ?? "check"
    }
    if (event.status === "failed" || event.status === "cancelled") {
      return wizardSteps.value.find(step => step.status === "active" || step.status === "failed")?.id ?? "check"
    }
    return "check"
  }

  function updateWizard(event: RuntimeEvent) {
    const targetId = stepIdForEvent(event)
    const targetIndex = wizardSteps.value.findIndex(step => step.id === targetId)
    if (targetIndex < 0)
      return
    const failed = event.status === "failed" || event.status === "cancelled"
    wizardSteps.value = wizardSteps.value.map((step, index) => {
      if (index < targetIndex && step.status !== "failed")
        return { ...step, status: "complete" }
      if (index !== targetIndex)
        return step
      if (failed)
        return { ...step, status: "failed" }
      if (event.status === "completed" || event.status === "ready" || event.status === "cached") {
        return { ...step, status: "complete" }
      }
      return { ...step, status: "active" }
    })
  }

  function applyRuntimeEvent(event: RuntimeEvent) {
    runtimeProgress.value = event.progress
    updateWizard(event)
    runtimeSnapshot.value = {
      ...runtimeSnapshot.value,
      status: event.status === "failed" || event.status === "cancelled" ? "failed" : "checking",
      message: event.message,
    }
    runtimeEvents.value = [...runtimeEvents.value, event].slice(-120)
  }

  function markWizardComplete() {
    wizardSteps.value = wizardSteps.value.map(step => ({ ...step, status: "complete" }))
  }

  function resetWizard() {
    wizardSteps.value = createDefaultWizardSteps(setupPlan.value)
  }

  async function loadSetupPlan(runtime: RuntimeKind) {
    if (!isTauri || selectedRuntime.value !== runtime)
      return
    try {
      const plan = await invoke<SetupPlanStep[]>("runtime_setup_plan", { runtime })
      if (selectedRuntime.value === runtime && Array.isArray(plan) && plan.length > 0) {
        setupPlan.value = plan
        if (!setupStarted.value && !runtimeActionRunning.value) {
          resetWizard()
        }
      }
    }
    catch {
      // Keep the bundled fallback plan when running an older backend binary.
    }
  }

  async function loadProjectCatalog(runtime: RuntimeKind) {
    if (!isTauri || selectedRuntime.value !== runtime)
      return
    try {
      const catalog = await invoke<ProjectInfo[]>("runtime_projects_catalog", { runtime })
      if (selectedRuntime.value === runtime && Array.isArray(catalog))
        projectCatalog.value = catalog
    }
    catch {
      if (selectedRuntime.value === runtime)
        projectCatalog.value = []
    }
  }

  async function refreshServices(runtime = selectedRuntime.value) {
    if (!isTauri || !runtime)
      return
    try {
      services.value = await invoke<ServiceSnapshot[]>("runtime_service_status", { runtime })
    }
    catch {
      services.value = []
    }
  }

  async function bootstrapRuntime() {
    const runtime = selectedRuntime.value
    if (runtimeActionRunning.value || !runtime)
      return
    setupStarted.value = true
    runtimeEvents.value = []
    resetWizard()
    runtimeActionRunning.value = true
    runtimeProgress.value = 3
    runtimeSnapshot.value = { ...runtimeSnapshot.value, status: "checking", selected_runtime: runtime, message: "正在检查 manifest、generation 和解释器…" }

    if (!isTauri) {
      const previewEvent: RuntimeEvent = {
        phase: "preview",
        status: "ready",
        message: "浏览器预览模式：等待 Tauri App 执行真实安装",
        progress: 100,
        timestamp: Math.floor(Date.now() / 1000),
        output: "preview only",
      }
      runtimeProgress.value = 100
      runtimeSnapshot.value = {
        ...runtimeSnapshot.value,
        status: "ready",
        platform: "web preview",
        arch: "browser",
        message: "浏览器预览不会启动本地服务；请使用 Tauri App 验证私有运行时。",
      }
      runtimeEvents.value = [previewEvent]
      markWizardComplete()
      runtimeActionRunning.value = false
      return
    }

    try {
      runtimeSnapshot.value = await invoke<RuntimeSnapshot>("runtime_bootstrap", { runtime })
      runtimeProgress.value = 100
      markWizardComplete()
      await refreshServices(runtime)
    }
    catch (error) {
      runtimeSnapshot.value = {
        ...runtimeSnapshot.value,
        status: "failed",
        message: error instanceof Error ? error.message : String(error),
      }
      if (!wizardSteps.value.some(step => step.status === "failed")) {
        const activeIndex = wizardSteps.value.findIndex(step => step.status === "active")
        const fallbackIndex = activeIndex >= 0 ? activeIndex : wizardSteps.value.length - 1
        wizardSteps.value = wizardSteps.value.map((step, index) => index === fallbackIndex ? { ...step, status: "failed" } : step)
      }
      const cancelled = String(error).includes("取消")
      runtimeEvents.value = [...runtimeEvents.value, {
        phase: "bootstrap",
        status: cancelled ? "cancelled" : "failed",
        message: runtimeSnapshot.value.message,
        progress: runtimeProgress.value,
        timestamp: Math.floor(Date.now() / 1000),
        output: cancelled ? "cancel requested" : "bootstrap exited with an error",
      }].slice(-120)
    }
    finally {
      runtimeActionRunning.value = false
    }
  }

  async function cancelRuntime() {
    if (!isTauri || !runtimeActionRunning.value)
      return
    try {
      await invoke("runtime_cancel")
      runtimeSnapshot.value = {
        ...runtimeSnapshot.value,
        message: "已发送取消请求，正在安全停止当前任务…",
      }
    }
    catch (error) {
      runtimeSnapshot.value = {
        ...runtimeSnapshot.value,
        message: error instanceof Error ? error.message : String(error),
      }
    }
  }

  async function repairRuntimeComponent(component: "python" | "node") {
    if (!isTauri || runtimeActionRunning.value || selectedRuntime.value !== component)
      return
    setupStarted.value = true
    runtimeEvents.value = []
    resetWizard()
    runtimeActionRunning.value = true
    runtimeProgress.value = 3
    runtimeSnapshot.value = {
      ...runtimeSnapshot.value,
      status: "checking",
      message: `正在单独修复 ${component === "python" ? "Python" : "Node.js"} 运行时…`,
    }
    try {
      runtimeSnapshot.value = await invoke<RuntimeSnapshot>("runtime_repair_component", { component })
      runtimeProgress.value = 100
      markWizardComplete()
      await refreshServices(component)
    }
    catch (error) {
      runtimeSnapshot.value = {
        ...runtimeSnapshot.value,
        status: "failed",
        message: error instanceof Error ? error.message : String(error),
      }
    }
    finally {
      runtimeActionRunning.value = false
    }
  }

  async function repairProjectEnvironment(projectId: string) {
    const runtime = selectedRuntime.value
    if (!isTauri || runtimeActionRunning.value || !runtime)
      return
    setupStarted.value = true
    runtimeEvents.value = []
    resetWizard()
    runtimeActionRunning.value = true
    runtimeProgress.value = 3
    runtimeSnapshot.value = {
      ...runtimeSnapshot.value,
      status: "checking",
      message: `正在修复 ${projectId} 项目环境…`,
    }
    try {
      await invoke<ProjectSnapshot>("runtime_project_repair", { projectId, runtime })
      runtimeSnapshot.value = await invoke<RuntimeSnapshot>("runtime_status", { runtime })
      runtimeProgress.value = 100
      markWizardComplete()
      await refreshServices(runtime)
    }
    catch (error) {
      runtimeSnapshot.value = {
        ...runtimeSnapshot.value,
        status: "failed",
        message: error instanceof Error ? error.message : String(error),
      }
    }
    finally {
      runtimeActionRunning.value = false
    }
  }

  function serviceFor(name: ServiceName) {
    return services.value.find(service => service.name === name)
  }

  async function callDemoService(name: ServiceName) {
    const service = serviceFor(name)
    if (!isTauri || !service?.running) {
      demoMessage.value = "示例服务尚未启动，请先完成运行时安装或修复。"
      return
    }
    const project = projectCatalog.value.find(item => item.project_id === name)
    if (!project?.demo_path) {
      demoMessage.value = "该项目未配置示例调用入口。"
      return
    }
    try {
      const response = await fetch(new URL(project.demo_path, `http://127.0.0.1:${service.port}`))
      const payload = await response.json() as { message?: string }
      demoMessage.value = payload.message ?? "服务响应成功。"
    }
    catch (error) {
      demoMessage.value = `服务调用失败：${error instanceof Error ? error.message : String(error)}`
    }
  }

  async function selectRuntime(runtime: RuntimeKind) {
    if (runtimeActionRunning.value)
      return
    selectedRuntime.value = runtime
    setupStarted.value = false
    runtimeProgress.value = 0
    runtimeEvents.value = []
    services.value = []
    projectCatalog.value = []
    runtimeSnapshot.value = initialSnapshot(runtime)
    setupPlan.value = createDefaultSetupPlan(runtime)
    resetWizard()
    await Promise.all([loadSetupPlan(runtime), loadProjectCatalog(runtime)])
  }

  onMounted(async () => {
    if (isTauri) {
      stopRuntimeEvents = await listen<RuntimeEvent>("runtime://event", event => applyRuntimeEvent(event.payload))
    }
  })

  onUnmounted(() => {
    stopRuntimeEvents?.()
  })

  return {
    isTauri,
    selectedRuntime,
    runtimeSnapshot,
    runtimeProgress,
    runtimeEvents,
    services,
    projectCatalog,
    runtimeActionRunning,
    setupStarted,
    demoMessage,
    wizardSteps,
    setupPlan,
    runtimeStatusLabel,
    runtimeDotClass,
    runtimePathLabel,
    selectRuntime,
    bootstrapRuntime,
    repairRuntimeComponent,
    repairProjectEnvironment,
    cancelRuntime,
    callDemoService,
  }
}
