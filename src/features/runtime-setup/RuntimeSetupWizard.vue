<script setup lang="ts">
import type { RuntimeEvent, RuntimeSnapshot, ServiceSnapshot, WizardStep } from "./types"
import {
  ArrowRight,
  Check,
  CircleAlert,
  CloudDownload,
  ExternalLink,
  LoaderCircle,
  LockKeyhole,
  Wrench,
  XCircle,
} from "@lucide/vue"
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue"
import logoUrl from "@/assets/tauri-embedded-runtime-logo.png"
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from "@/components/ui/accordion"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
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
}>()

const emit = defineEmits<{
  run: []
  cancel: []
  openBusiness: []
}>()

const expandedStepId = ref<string | undefined>()
const scrollAreaHost = ref<HTMLElement | null>(null)
const followLatestLogs = ref(true)
let logViewport: HTMLElement | null = null
const isLanding = computed(() => !props.setupStarted)
const canOpenBusiness = computed(() => props.snapshot.status === "ready" && props.services.some(service => service.running))
const activeStepIndex = computed(() => {
  const active = props.steps.findIndex(step => step.status === "active" || step.status === "failed")
  if (active >= 0)
    return active
  const pending = props.steps.findIndex(step => step.status === "pending")
  return pending >= 0 ? pending : props.steps.length - 1
})
const activeStep = computed(() => props.steps[activeStepIndex.value])
const completedSteps = computed(() => props.steps.filter(step => step.status === "complete").length)
const stepSignature = computed(() => props.steps.map(step => `${step.id}:${step.status}`).join("|"))

watch([stepSignature, () => props.actionRunning], ([, running]) => {
  if (!running) {
    expandedStepId.value = undefined
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
  const viewport = scrollAreaHost.value?.querySelector<HTMLElement>(
    "[data-slot=\"scroll-area-viewport\"]",
  )
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

function stepEvents(stepId: string) {
  return props.events.filter(event => stepIdForEvent(event) === stepId)
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
  <main class="flex min-h-0 flex-1 flex-col bg-background text-foreground">
    <section v-if="isLanding" class="flex min-h-0 flex-1 flex-col items-center justify-center px-7 text-center">
      <img :src="logoUrl" alt="Tauri Embedded Runtime Logo" class="size-16 rounded-2xl shadow-lg shadow-primary/15">
      <p class="mt-5 text-[9px] font-semibold uppercase tracking-[0.26em] text-muted-foreground">
        Hermes runtime installer
      </p>
      <h1 class="mt-3 max-w-xl text-3xl font-semibold tracking-[-0.06em] text-primary sm:text-4xl">
        运行环境检测
      </h1>
      <p class="mt-4 max-w-lg text-sm leading-6 text-muted-foreground">
        手动检查并准备应用私有目录中的 Python、Node.js 及各服务依赖。不会修改宿主机 PATH 或 shell 配置。
      </p>
      <Button class="mt-7 h-10 rounded-lg px-5 text-sm shadow-md shadow-primary/20" @click="emit('run')">
        运行检测
        <ArrowRight class="size-4" />
      </Button>
      <div class="mt-8 flex flex-wrap items-center justify-center gap-x-5 gap-y-2 text-[10px] text-muted-foreground">
        <span class="flex items-center gap-1.5"><LockKeyhole class="size-3 text-emerald-500" />App 私有目录</span>
        <span class="flex items-center gap-1.5"><CloudDownload class="size-3 text-primary" />固定版本</span>
        <span class="flex items-center gap-1.5"><Wrench class="size-3 text-amber-500" />可修复</span>
      </div>
    </section>

    <template v-else>
      <header class="shrink-0 border-b border-border bg-background px-5 pb-4 pt-4 sm:px-7 lg:px-9">
        <div class="flex items-center justify-between gap-3">
          <div class="flex min-w-0 items-center gap-2.5">
            <LoaderCircle v-if="props.actionRunning" class="size-4 shrink-0 animate-spin text-primary" />
            <Check v-else-if="props.snapshot.status === 'ready'" class="size-4 shrink-0 text-emerald-600" />
            <CircleAlert v-else class="size-4 shrink-0 text-amber-500" />
            <h1 class="truncate text-base font-medium tracking-[-0.03em] text-foreground sm:text-lg">
              {{ props.actionRunning ? activeStep?.title : props.snapshot.status === 'ready' ? '运行环境已就绪' : activeStep?.title }}
            </h1>
          </div>
          <span class="shrink-0 text-xs text-muted-foreground">{{ completedSteps }} of {{ props.steps.length }} steps</span>
        </div>
        <Progress :model-value="props.progress" class="mt-3 h-1.5 bg-secondary [&_[data-slot=progress-indicator]]:bg-primary" />
      </header>

      <div ref="scrollAreaHost" class="min-h-0 flex-1">
        <ScrollArea class="h-full" type="always">
          <div class="px-5 py-4 sm:px-7 sm:py-5 lg:px-9">
            <Card size="sm" class="mx-auto w-full max-w-none bg-card py-1 shadow-sm ring-border">
              <CardContent class="px-3 sm:px-4">
                <Accordion v-model="expandedStepId" type="single" collapsible>
                  <AccordionItem v-for="step in props.steps" :key="step.id" :value="step.id">
                    <AccordionTrigger class="gap-2.5 px-2 py-3 hover:no-underline" :class="step.status === 'active' ? 'bg-accent' : ''">
                      <span class="flex size-4 shrink-0 items-center justify-center">
                        <LoaderCircle v-if="step.status === 'active'" class="size-3.5 animate-spin text-primary" />
                        <Check v-else-if="step.status === 'complete'" class="size-3.5 text-primary" />
                        <XCircle v-else-if="step.status === 'failed'" class="size-3.5 text-red-500" />
                        <span v-else class="size-1.5 rounded-full bg-muted-foreground/40" />
                      </span>
                      <span class="min-w-0 flex-1 text-xs font-medium sm:text-sm" :class="step.status === 'pending' ? 'text-muted-foreground' : 'text-foreground'">{{ step.title }}</span>
                      <Badge variant="outline" class="mr-1 h-5 px-1.5 text-[9px] font-normal" :class="stepStatusClass(step.status)">
                        {{ stepStatusLabel(step.status) }}
                      </Badge>
                    </AccordionTrigger>

                    <AccordionContent class="px-0">
                      <div class="mb-2 ml-6 mr-2 rounded-lg border border-border bg-muted/30 px-3 py-2.5">
                        <template v-if="stepEvents(step.id).length">
                          <div v-for="(event, eventIndex) in stepEvents(step.id)" :key="`${event.timestamp}-${eventIndex}`" class="font-mono text-[10px] leading-5" :class="eventClass(event)">
                            <div><span class="text-muted-foreground/50">[{{ eventTime(event) }}]</span> {{ event.message }}</div>
                            <div v-if="event.output" class="truncate pl-3 text-muted-foreground" :title="event.output">
                              ↳ {{ event.output }}
                            </div>
                          </div>
                        </template>
                        <p v-else class="text-[10px] text-muted-foreground">
                          {{ step.status === 'active' ? props.snapshot.message : step.status === 'complete' ? '该步骤已完成，暂无新的输出。' : step.status === 'failed' ? '该步骤执行失败，请重新检查或修复。' : '等待该步骤执行…' }}
                        </p>
                      </div>
                    </AccordionContent>
                  </AccordionItem>
                </Accordion>
              </CardContent>
            </Card>
          </div>
        </ScrollArea>
      </div>

      <footer class="flex shrink-0 justify-end gap-3 border-t border-border bg-background px-5 py-3 sm:px-7 lg:px-9">
        <div class="flex items-center gap-2">
          <Button v-if="props.actionRunning" variant="outline" class="h-8 px-4 text-xs" @click="emit('cancel')">
            取消
          </Button>
          <template v-else>
            <Button v-if="canOpenBusiness" variant="outline" class="h-8 px-4 text-xs text-primary" @click="emit('openBusiness')">
              打开业务首页
              <ExternalLink class="size-3.5" />
            </Button>
            <Button class="h-8 px-4 text-xs" @click="emit('run')">
              {{ props.snapshot.status === 'ready' ? '再次运行检测' : props.snapshot.status === 'failed' || props.snapshot.status === 'corrupted' ? '修复并启动' : '运行检测' }}
              <ArrowRight class="size-3.5" />
            </Button>
          </template>
        </div>
      </footer>
    </template>
  </main>
</template>
