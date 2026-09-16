<script setup lang="ts">
import type { RuntimeEvent, RuntimeKind, RuntimeSnapshot, ServiceSnapshot, WizardStep } from "./types"
import {
  ArrowRight,
  Braces,
  Check,
  CircleAlert,
  CloudDownload,
  Code2,
  ExternalLink,
  LoaderCircle,
  LockKeyhole,
  ShieldCheck,
  XCircle,
} from "@lucide/vue"
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue"
import RuntimeAppHeader from "@/components/runtime/RuntimeAppHeader.vue"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Progress } from "@/components/ui/progress"
import { ScrollArea } from "@/components/ui/scroll-area"

const props = defineProps<{
  snapshot: RuntimeSnapshot
  progress: number
  events: RuntimeEvent[]
  steps: WizardStep[]
  actionRunning: boolean
  setupStarted: boolean
  services: ServiceSnapshot[]
  selectedRuntime: RuntimeKind | null
}>()

const emit = defineEmits<{
  run: []
  cancel: []
  openBusiness: []
  selectRuntime: [runtime: RuntimeKind]
}>()

const expandedStepId = ref<string | undefined>()
const logPanel = ref<HTMLElement | null>(null)
const followLatestLogs = ref(true)
let logViewport: HTMLElement | null = null
const isLanding = computed(() => !props.setupStarted)
const canOpenBusiness = computed(() => props.snapshot.status === "ready" && props.services.some(service => service.running))
const runtimeOptions = [
  {
    id: "python" as RuntimeKind,
    title: "Python",
    description: "适合数据处理、AI 与自动化任务",
    detail: "CPython 与 Python 项目环境",
    icon: Code2,
  },
  {
    id: "node" as RuntimeKind,
    title: "Node.js",
    description: "适合 Web 服务与前端工具链",
    detail: "Node.js 与 Node 项目环境",
    icon: Braces,
  },
] as const
const selectedRuntimeLabel = computed(() => props.selectedRuntime === "node" ? "Node.js" : "Python")
const activeStepIndex = computed(() => {
  const active = props.steps.findIndex(step => step.status === "active" || step.status === "failed")
  if (active >= 0)
    return active
  const pending = props.steps.findIndex(step => step.status === "pending")
  return pending >= 0 ? pending : props.steps.length - 1
})
const activeStep = computed(() => props.steps[activeStepIndex.value])
const visibleStep = computed(() => props.steps.find(step => step.id === expandedStepId.value) ?? activeStep.value)
const visibleStepIndex = computed(() => props.steps.findIndex(step => step.id === visibleStep.value?.id))
const completedSteps = computed(() => props.steps.filter(step => step.status === "complete").length)
const progressLabel = computed(() => {
  if (!props.setupStarted)
    return `准备 ${props.steps.length} 个步骤`
  if (!props.actionRunning && props.snapshot.status === "ready")
    return `${props.steps.length} / ${props.steps.length} 步已完成`
  if (props.snapshot.status === "failed")
    return `${completedSteps.value} / ${props.steps.length} 步已完成`
  return `第 ${Math.min(activeStepIndex.value + 1, props.steps.length)} / ${props.steps.length} 步`
})
const stepSignature = computed(() => props.steps.map(step => `${step.id}:${step.status}`).join("|"))
const setupTitle = computed(() => {
  if (!props.actionRunning && props.snapshot.status === "ready")
    return "运行环境已就绪"
  if (props.actionRunning)
    return "正在准备运行时环境"
  return "检查运行环境"
})
const setupDescription = computed(() => {
  if (!props.actionRunning && props.snapshot.status === "ready")
    return `${selectedRuntimeLabel.value} 环境已经准备完成，可以开始使用本地服务。`
  if (props.snapshot.status === "failed")
    return "准备过程中遇到问题，请查看下方日志后重试。"
  return "这只需要几分钟，请保持应用打开。"
})

watch([stepSignature, () => props.actionRunning], ([, running]) => {
  if (!running) {
    expandedStepId.value = props.steps.find(step => step.status === "failed")?.id
      ?? props.steps[props.steps.length - 1]?.id
    return
  }
  followLatestLogs.value = true
  expandedStepId.value = props.steps.find(step => step.status === "active")?.id
}, { immediate: true })

function isNearLogBottom(viewport: HTMLElement) {
  return viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight < 24
}

function onLogViewportScroll() {
  if (logViewport) {
    followLatestLogs.value = isNearLogBottom(logViewport)
  }
}

function bindLogViewport() {
  const viewport = logPanel.value?.querySelector<HTMLElement>("[data-runtime-log]")
  if (viewport === logViewport)
    return
  logViewport?.removeEventListener("scroll", onLogViewportScroll)
  logViewport = viewport ?? null
  logViewport?.addEventListener("scroll", onLogViewportScroll, { passive: true })
}

async function scrollLogsToLatest() {
  await nextTick()
  bindLogViewport()
  if (!logViewport || !followLatestLogs.value)
    return
  logViewport.scrollTo({ top: logViewport.scrollHeight, behavior: "auto" })
}

watch(
  [() => props.events.length, expandedStepId, () => props.setupStarted],
  scrollLogsToLatest,
  { flush: "post" },
)

onMounted(scrollLogsToLatest)
onUnmounted(() => logViewport?.removeEventListener("scroll", onLogViewportScroll))

function stepIdForEvent(event: RuntimeEvent) {
  if (event.phase === "cancel" || event.phase === "cancelled") {
    return props.steps.find(step => step.status === "active")?.id ?? "check"
  }
  if (props.steps.some(step => step.id === event.phase))
    return event.phase
  if (event.phase === "download" || event.phase === "extract") {
    return props.steps.find(step => step.status === "active")?.id ?? "check"
  }
  if (event.phase === "services" || event.phase === "complete") {
    return props.steps.find(step => step.id === "verify")?.id
      ?? props.steps[props.steps.length - 1]?.id
      ?? "check"
  }
  if (event.phase === "install") {
    return props.steps.find(step => step.status === "active" || step.status === "failed")?.id ?? "check"
  }
  if (event.status === "failed" || event.status === "cancelled") {
    return props.steps.find(step => step.status === "active" || step.status === "failed")?.id ?? "check"
  }
  return "check"
}

function isTerminalEvent(event: RuntimeEvent) {
  return event.status === "completed"
    || event.status === "ready"
    || event.status === "cached"
    || event.status === "failed"
    || event.status === "cancelled"
}

function stepEvents(stepId: string) {
  const events = props.events.filter(event => stepIdForEvent(event) === stepId)
  return events.filter((event, index) => event.status !== "running"
    || !events.slice(index + 1).some(nextEvent => nextEvent.phase === event.phase
      && nextEvent.operation_id === event.operation_id
      && isTerminalEvent(nextEvent)))
}

function selectStep(stepId: string) {
  expandedStepId.value = stepId
  followLatestLogs.value = props.steps.find(step => step.id === stepId)?.status === "active"
}

function stepStatusLabel(status: WizardStep["status"]) {
  if (status === "complete")
    return "完成"
  if (status === "active")
    return "进行中"
  if (status === "failed")
    return "失败"
  return "等待中"
}

function stepStatusClass(status: WizardStep["status"]) {
  if (status === "complete")
    return "text-emerald-600"
  if (status === "active")
    return "text-primary"
  if (status === "failed")
    return "text-red-500"
  return "text-muted-foreground"
}

function eventClass(event: RuntimeEvent) {
  if (event.status === "failed" || event.status === "cancelled")
    return "text-red-600"
  if (event.status === "completed" || event.status === "ready" || event.status === "cached")
    return "text-emerald-600"
  return "text-muted-foreground"
}

function eventTime(event: RuntimeEvent) {
  return new Date(event.timestamp * 1000).toLocaleTimeString([], { hour12: false })
}
</script>

<template>
  <main class="runtime-page">
    <RuntimeAppHeader>
      <span class="shrink-0 text-[10px] text-muted-foreground">首次设置</span>
    </RuntimeAppHeader>

    <section v-if="isLanding" class="min-h-0 flex-1">
      <ScrollArea class="h-full">
        <div class="runtime-page-content flex min-h-[calc(100dvh-3rem)] flex-col justify-center py-5 sm:py-6 lg:py-10 xl:min-h-[calc(100dvh-3.5rem)] xl:py-16">
          <div class="grid gap-7 sm:grid-cols-[minmax(0,0.82fr)_minmax(320px,1.18fr)] sm:items-center sm:gap-6 lg:gap-10 xl:gap-16">
            <section class="max-w-xl">
              <p class="text-[10px] font-medium text-muted-foreground lg:text-xs">
                欢迎使用
              </p>
              <h1 class="mt-1.5 text-xl font-semibold tracking-[-0.065em] text-foreground lg:mt-2 lg:text-3xl xl:mt-3 xl:text-5xl">
                Tauri Embedded Runtime
              </h1>
              <p class="mt-2 text-xs leading-5 text-foreground/80 lg:mt-3 lg:text-base lg:leading-7 xl:mt-4 xl:text-lg xl:leading-8">
                为你的桌面应用准备一个私有的运行时环境
              </p>
              <p class="mt-2.5 max-w-lg text-[11px] leading-5 text-muted-foreground lg:mt-4 lg:text-xs lg:leading-6 xl:mt-5 xl:text-sm xl:leading-7">
                在你的设备上创建独立的 Python 或 Node.js 运行时，并启动本地服务。我们不会修改系统的 PATH，也不会安装任何全局包。
              </p>

              <div class="mt-5 space-y-3 border-t border-border pt-4 lg:mt-8 lg:space-y-4 lg:pt-6 xl:mt-10 xl:space-y-5 xl:pt-7">
                <div class="flex items-start gap-3">
                  <LockKeyhole class="mt-0.5 size-3 shrink-0 text-primary lg:size-3.5 xl:size-4" />
                  <div>
                    <p class="text-[11px] font-medium lg:text-xs xl:text-sm">
                      完全私有
                    </p>
                    <p class="mt-0.5 text-[9px] leading-4 text-muted-foreground lg:mt-1 lg:text-[10px] lg:leading-5 xl:text-xs">
                      运行时安装在应用数据目录中，与系统环境隔离。
                    </p>
                  </div>
                </div>
                <div class="flex items-start gap-3">
                  <ShieldCheck class="mt-0.5 size-3 shrink-0 text-primary lg:size-3.5 xl:size-4" />
                  <div>
                    <p class="text-[11px] font-medium lg:text-xs xl:text-sm">
                      不修改系统
                    </p>
                    <p class="mt-0.5 text-[9px] leading-4 text-muted-foreground lg:mt-1 lg:text-[10px] lg:leading-5 xl:text-xs">
                      不改变 PATH，不安装全局包，不影响你已有的开发环境。
                    </p>
                  </div>
                </div>
                <div class="flex items-start gap-3">
                  <CloudDownload class="mt-0.5 size-3 shrink-0 text-primary lg:size-3.5 xl:size-4" />
                  <div>
                    <p class="text-[11px] font-medium lg:text-xs xl:text-sm">
                      开箱即用
                    </p>
                    <p class="mt-0.5 text-[9px] leading-4 text-muted-foreground lg:mt-1 lg:text-[10px] lg:leading-5 xl:text-xs">
                      安装完成后自动配置本地服务，打开应用即可使用。
                    </p>
                  </div>
                </div>
              </div>
            </section>

            <section class="rounded-2xl border border-border bg-card p-4 shadow-sm lg:p-5 xl:p-7">
              <div class="flex items-start justify-between gap-4">
                <div>
                  <h2 class="text-sm font-semibold tracking-[-0.04em] lg:text-lg xl:text-xl">
                    选择要安装的运行时
                  </h2>
                  <p class="mt-1 text-[10px] leading-4 text-muted-foreground lg:mt-1.5 lg:text-xs lg:leading-5 xl:mt-2 xl:leading-5">
                    选择一个环境，应用会准备与之匹配的本地服务。
                  </p>
                </div>
                <span class="hidden shrink-0 text-[10px] text-muted-foreground sm:block">自动完成</span>
              </div>

              <div class="mt-3.5 space-y-2 lg:mt-5 lg:space-y-2.5 xl:mt-6 xl:space-y-3">
                <button
                  v-for="option in runtimeOptions"
                  :key="option.id"
                  type="button"
                  class="group flex w-full items-center gap-2 rounded-xl border px-3 py-2.5 text-left transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring lg:gap-3 lg:px-4 lg:py-3 xl:py-4"
                  :class="props.selectedRuntime === option.id ? 'border-primary bg-accent' : 'border-border'"
                  @click="emit('selectRuntime', option.id)"
                >
                  <span class="flex size-4 shrink-0 items-center justify-center rounded-full border lg:size-5" :class="props.selectedRuntime === option.id ? 'border-primary' : 'border-muted-foreground/40'">
                    <span v-if="props.selectedRuntime === option.id" class="size-2 rounded-full bg-primary xl:size-2.5" />
                  </span>
                  <span class="flex size-7 shrink-0 items-center justify-center rounded-lg bg-muted text-primary lg:size-9 xl:size-10">
                    <component :is="option.icon" class="size-3.5 lg:size-4 xl:size-5" />
                  </span>
                  <span class="min-w-0 flex-1">
                    <span class="block text-[11px] font-medium lg:text-xs xl:text-sm">{{ option.title }}</span>
                    <span class="mt-0.5 block text-[9px] leading-4 text-muted-foreground lg:mt-1 lg:text-[10px] lg:leading-5 xl:text-xs">{{ option.description }}</span>
                    <span class="mt-0.5 block text-[8px] text-muted-foreground/70 lg:mt-1 lg:text-[9px] xl:text-[10px]">{{ option.detail }}</span>
                  </span>
                  <ArrowRight class="size-3 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5 lg:size-3.5 xl:size-4" />
                </button>
              </div>

              <div class="mt-3 flex items-start gap-2 rounded-xl bg-muted/50 px-3 py-2.5 lg:mt-4 lg:gap-3 lg:px-4 lg:py-3 xl:mt-5 xl:py-3.5">
                <LockKeyhole class="mt-0.5 size-3 shrink-0 text-primary lg:size-3.5 xl:size-4" />
                <div>
                  <p class="text-[10px] font-medium lg:text-[11px] xl:text-xs">
                    你的环境，始终私有
                  </p>
                  <p class="mt-0.5 text-[9px] leading-4 text-muted-foreground lg:mt-1 lg:text-[10px] lg:leading-5 xl:text-[11px]">
                    所有文件仅存放在此应用的私有目录中，不会改动系统 PATH，也不会影响其他项目。
                  </p>
                </div>
              </div>

              <Button class="mt-3 h-8 w-full gap-2 rounded-lg text-[10px] lg:mt-4 lg:h-9 lg:text-xs xl:mt-6 xl:h-11 xl:text-sm" :disabled="!props.selectedRuntime" @click="emit('run')">
                继续
                <ArrowRight class="size-4" />
              </Button>
              <p class="mt-1.5 text-center text-[8px] text-muted-foreground lg:mt-2 lg:text-[9px] xl:mt-3 xl:text-[10px]">
                {{ props.selectedRuntime ? `将准备 ${selectedRuntimeLabel} 运行环境` : "请选择一个运行时后继续" }}
              </p>
            </section>
          </div>
        </div>
      </ScrollArea>
    </section>

    <template v-else>
      <section class="shrink-0 border-b border-border bg-background">
        <div class="mx-auto flex w-full max-w-6xl flex-col gap-2 px-4 py-3 sm:flex-row sm:items-center sm:justify-between sm:px-6 sm:py-3.5 lg:gap-4 lg:px-8 lg:py-5 xl:gap-6 xl:px-10 xl:py-8">
          <div class="min-w-0">
            <div class="flex items-center gap-1.5 text-[9px] text-muted-foreground lg:gap-2 lg:text-[10px] xl:text-xs">
              <LoaderCircle v-if="props.actionRunning" class="size-3 animate-spin text-primary lg:size-3.5 xl:size-3.5" />
              <Check v-else-if="props.snapshot.status === 'ready'" class="size-3 text-emerald-600 lg:size-3.5 xl:size-3.5" />
              <CircleAlert v-else class="size-3 text-amber-500 lg:size-3.5 xl:size-3.5" />
              <span>{{ selectedRuntimeLabel }} 运行时</span>
            </div>
            <h1 class="mt-1 text-lg font-semibold tracking-[-0.06em] lg:mt-1.5 lg:text-2xl xl:mt-3 xl:text-4xl">
              {{ setupTitle }}
            </h1>
            <p class="mt-0.5 text-[10px] leading-4 text-muted-foreground lg:mt-1 lg:text-xs lg:leading-5 xl:mt-2 xl:text-sm xl:leading-6">
              {{ setupDescription }}
            </p>
          </div>
          <div class="w-full shrink-0 sm:w-[240px] lg:w-[280px] xl:w-[360px]">
            <div class="flex items-center justify-between text-[9px] text-muted-foreground lg:text-[10px] xl:text-xs">
              <span>{{ progressLabel }}</span>
              <span class="font-mono tabular-nums">{{ props.progress }}%</span>
            </div>
            <Progress :model-value="props.progress" aria-label="运行时准备进度" class="mt-1 h-1 bg-secondary [&_[data-slot=progress-indicator]]:bg-primary lg:mt-1.5 lg:h-1.5 xl:mt-2" />
          </div>
        </div>
      </section>

      <div class="min-h-0 flex-1">
        <ScrollArea class="h-full" type="always">
          <div class="runtime-page-content grid min-h-[calc(100dvh-11rem)] gap-5 py-5 pb-20 sm:grid-cols-[minmax(210px,0.34fr)_minmax(0,0.66fr)] sm:gap-6 sm:py-6 sm:pb-24 lg:min-h-[calc(100dvh-13rem)] lg:gap-8 lg:py-8 lg:pb-24 xl:min-h-[calc(100dvh-18rem)] xl:gap-10 xl:py-12 xl:pb-28">
            <nav aria-label="运行时准备步骤" class="min-w-0">
              <div class="mb-2.5 flex items-center justify-between lg:mb-4 xl:mb-5">
                <p class="text-[10px] font-medium text-muted-foreground lg:text-[11px] xl:text-xs">
                  准备步骤
                </p>
                <Badge variant="outline" class="h-4 px-1.5 text-[8px] font-normal lg:h-5 lg:text-[9px]">
                  {{ selectedRuntimeLabel }}
                </Badge>
              </div>
              <div class="space-y-1">
                <button
                  v-for="(step, index) in props.steps"
                  :key="step.id"
                  type="button"
                  class="group relative flex w-full items-start gap-2 rounded-xl px-2 py-1.5 text-left transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring lg:gap-3 lg:px-3 lg:py-2.5 xl:py-3"
                  :class="visibleStep?.id === step.id ? 'bg-accent' : ''"
                  @click="selectStep(step.id)"
                >
                  <span class="relative z-10 flex size-6 shrink-0 items-center justify-center rounded-full border text-[9px] font-medium lg:size-7 lg:text-[10px] xl:size-8 xl:text-xs" :class="step.status === 'complete' ? 'border-primary bg-primary text-primary-foreground' : step.status === 'active' ? 'border-primary bg-primary text-primary-foreground' : step.status === 'failed' ? 'border-destructive bg-destructive text-destructive-foreground' : 'border-border bg-background text-muted-foreground'">
                    <LoaderCircle v-if="step.status === 'active'" class="size-2.5 animate-spin lg:size-3 xl:size-3.5" />
                    <Check v-else-if="step.status === 'complete'" class="size-2.5 lg:size-3 xl:size-3.5" />
                    <XCircle v-else-if="step.status === 'failed'" class="size-2.5 lg:size-3 xl:size-3.5" />
                    <span v-else>{{ index + 1 }}</span>
                  </span>
                  <span class="min-w-0 flex-1 pt-0.5">
                    <span class="flex items-center justify-between gap-2">
                      <span class="truncate text-[11px] font-medium lg:text-xs xl:text-sm" :class="step.status === 'pending' ? 'text-muted-foreground' : 'text-foreground'">{{ step.title }}</span>
                      <span class="shrink-0 text-[8px] lg:text-[9px] xl:text-[10px]" :class="stepStatusClass(step.status)">{{ stepStatusLabel(step.status) }}</span>
                    </span>
                    <span class="mt-0.5 block text-[9px] leading-4 text-muted-foreground lg:mt-1 lg:text-[10px] lg:leading-5 xl:text-[11px]">{{ step.description }}</span>
                  </span>
                  <span v-if="index < props.steps.length - 1" class="absolute left-[1.15rem] top-10 h-[calc(100%-1rem)] w-px bg-border" />
                </button>
              </div>
            </nav>

            <section v-if="visibleStep" class="min-w-0 border-t border-border pt-5 sm:flex sm:min-h-[280px] sm:flex-col sm:border-l sm:border-t-0 sm:pl-6 sm:pt-0 lg:min-h-[320px] lg:pl-8 xl:pl-10">
              <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between lg:gap-5">
                <div class="min-w-0">
                  <p class="text-[9px] font-medium uppercase tracking-[0.18em] text-muted-foreground lg:text-[10px]">
                    步骤 {{ visibleStepIndex + 1 }} / {{ props.steps.length }}
                  </p>
                  <h2 class="mt-1 text-lg font-semibold tracking-[-0.05em] lg:mt-1.5 lg:text-xl xl:mt-2 xl:text-2xl">
                    {{ visibleStep.title }}
                  </h2>
                  <p class="mt-1 text-[11px] leading-4 text-muted-foreground lg:mt-1.5 lg:text-xs lg:leading-5 xl:mt-2 xl:text-sm xl:leading-6">
                    {{ visibleStep.description }}
                  </p>
                </div>
                <Badge variant="outline" class="h-5 shrink-0 px-2 text-[8px] font-normal lg:text-[9px] xl:h-6 xl:text-[10px]" :class="stepStatusClass(visibleStep.status)">
                  {{ stepStatusLabel(visibleStep.status) }}
                </Badge>
              </div>

              <div class="mt-5 border-t border-border pt-4 lg:mt-6 lg:pt-5 xl:mt-8 xl:pt-6">
                <p class="text-[11px] font-medium lg:text-xs xl:text-sm">
                  {{ props.actionRunning && visibleStep.status === 'active' ? props.snapshot.message : visibleStep.status === 'complete' ? '该步骤已完成' : visibleStep.status === 'failed' ? '该步骤执行失败，请查看日志' : props.actionRunning ? '正在等待前置步骤完成…' : '等待该步骤执行' }}
                </p>
                <p class="mt-0.5 text-[9px] text-muted-foreground lg:mt-1 lg:text-[10px] xl:text-xs">
                  实时输出
                </p>
              </div>

              <div ref="logPanel" class="mt-2.5 overflow-hidden rounded-xl border border-border bg-muted/30 sm:flex sm:min-h-0 sm:flex-1 sm:flex-col lg:mt-3 xl:mt-4">
                <div data-runtime-log class="max-h-48 min-h-28 overflow-y-auto px-3 py-3 sm:h-full sm:min-h-0 sm:w-full sm:max-h-none sm:flex-1 sm:px-4 lg:max-h-96 lg:px-5 lg:py-4 xl:max-h-[28rem]">
                  <template v-if="stepEvents(visibleStep.id).length">
                    <div v-for="(event, eventIndex) in stepEvents(visibleStep.id)" :key="`${event.timestamp}-${eventIndex}`" class="flex gap-2 font-mono text-[8px] leading-4 xl:gap-3 xl:text-[10px] xl:leading-5" :class="eventClass(event)">
                      <span class="shrink-0 text-muted-foreground/60">{{ eventTime(event) }}</span>
                      <span class="min-w-0 flex-1">
                        <span class="block">{{ event.message }}</span>
                        <span v-if="event.output" class="block truncate pl-2 text-muted-foreground" :title="event.output">↳ {{ event.output }}</span>
                      </span>
                    </div>
                  </template>
                  <p v-else class="flex min-h-32 items-center justify-center text-center text-xs text-muted-foreground sm:h-full sm:min-h-0">
                    {{ visibleStep.status === 'pending' ? (props.actionRunning ? '即将开始执行…' : '等待该步骤执行…') : '暂无新的输出。' }}
                  </p>
                </div>
              </div>
            </section>
          </div>
        </ScrollArea>
      </div>

      <footer class="flex shrink-0 items-center justify-between gap-3 border-t border-border bg-background px-4 py-2.5 sm:px-6 xl:px-10 xl:py-4">
        <Button v-if="props.actionRunning" variant="outline" class="h-7 px-3 text-[9px] xl:h-9 xl:px-4 xl:text-xs" @click="emit('cancel')">
          取消
        </Button>
        <span v-else class="text-[9px] text-muted-foreground xl:text-xs">
          {{ props.snapshot.status === 'ready' ? '运行环境已准备完成' : '可以重新运行检测' }}
        </span>
        <div class="flex items-center gap-2">
          <Button v-if="props.actionRunning" variant="outline" class="h-7 gap-2 px-3 text-[9px] xl:h-9 xl:px-4 xl:text-xs" disabled>
            继续
            <ArrowRight class="size-3.5" />
          </Button>
          <template v-else>
            <Button v-if="canOpenBusiness" variant="outline" class="h-7 gap-2 px-3 text-[9px] text-primary xl:h-9 xl:px-4 xl:text-xs" @click="emit('openBusiness')">
              打开业务首页
              <ExternalLink class="size-3.5" />
            </Button>
            <Button class="h-7 gap-2 px-3 text-[9px] xl:h-9 xl:px-4 xl:text-xs" @click="emit('run')">
              {{ props.snapshot.status === 'ready' ? '再次运行检测' : props.snapshot.status === 'failed' || props.snapshot.status === 'corrupted' ? '修复并启动' : '运行检测' }}
              <ArrowRight class="size-3.5" />
            </Button>
          </template>
        </div>
      </footer>
    </template>
  </main>
</template>
