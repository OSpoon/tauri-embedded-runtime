<script setup lang="ts">
import type { ProjectInfo, ServiceName, ServiceSnapshot } from "@/features/runtime-setup/types"
import { Activity, ArrowLeft, ArrowRight, CircleAlert, ExternalLink, Server } from "@lucide/vue"
import { computed } from "vue"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { ScrollArea } from "@/components/ui/scroll-area"

const props = defineProps<{
  services: ServiceSnapshot[]
  projects: ProjectInfo[]
  message: string
}>()

const emit = defineEmits<{
  back: []
  call: [name: ServiceName]
}>()

const runningCount = computed(() => props.projects.filter(project => serviceFor(project.project_id)?.running).length)

function serviceFor(name: ServiceName) {
  return props.services.find(service => service.name === name)
}
</script>

<template>
  <main class="flex min-h-0 flex-1 flex-col bg-[#f8f9fd] text-zinc-900">
    <ScrollArea class="h-full">
      <div class="mx-auto w-full max-w-5xl px-5 py-5 sm:px-8 sm:py-7 lg:px-10">
        <header class="flex flex-wrap items-start justify-between gap-4">
          <div class="min-w-0">
            <p class="text-[10px] font-semibold uppercase tracking-[0.24em] text-blue-600">
              Local business workspace
            </p>
            <h1 class="mt-2 text-2xl font-semibold tracking-[-0.05em] text-zinc-900 sm:text-3xl">
              业务首页
            </h1>
            <p class="mt-2 max-w-2xl text-sm leading-6 text-zinc-500">
              运行环境已经准备完成。你可以从这里调用已启动的本地服务，作为后续真实项目接入的入口。
            </p>
          </div>
          <Button variant="outline" size="sm" class="shrink-0 bg-white" @click="emit('back')">
            <ArrowLeft class="size-4" />
            返回运行检测
          </Button>
        </header>

        <Card class="mt-6 bg-white/80 shadow-sm shadow-zinc-900/[0.03]">
          <CardHeader class="pb-3">
            <div class="flex items-center justify-between gap-3">
              <div>
                <CardTitle class="text-base">
                  本地服务
                </CardTitle>
                <CardDescription class="mt-1">
                  服务由 App 私有运行时托管，不依赖宿主机环境。
                </CardDescription>
              </div>
              <Badge variant="outline" class="shrink-0 gap-1.5 text-xs font-normal text-emerald-600">
                <Activity class="size-3" />
                {{ runningCount }} / {{ props.projects.length }} 运行中
              </Badge>
            </div>
          </CardHeader>
          <CardContent>
            <div v-if="props.projects.length" class="grid gap-3 sm:grid-cols-2">
              <Card v-for="project in props.projects" :key="project.project_id" class="bg-[#fafbfe] shadow-none ring-zinc-200">
                <CardHeader class="pb-3">
                  <div class="flex items-start justify-between gap-3">
                    <div class="flex min-w-0 items-center gap-3">
                      <div class="flex size-9 shrink-0 items-center justify-center rounded-lg bg-blue-50 text-blue-600">
                        <Server class="size-4" />
                      </div>
                      <div class="min-w-0">
                        <CardTitle class="text-sm">
                          {{ project.display_name }}
                        </CardTitle>
                        <CardDescription class="mt-0.5 text-xs">
                          {{ project.service_label }}
                        </CardDescription>
                      </div>
                    </div>
                    <Badge v-if="serviceFor(project.project_id)?.running" variant="outline" class="shrink-0 text-[10px] font-normal text-emerald-600">
                      运行中
                    </Badge>
                    <Badge v-else variant="outline" class="shrink-0 text-[10px] font-normal text-zinc-400">
                      未启动
                    </Badge>
                  </div>
                </CardHeader>
                <CardContent class="space-y-3">
                  <p class="text-xs leading-5 text-zinc-500">
                    {{ project.framework }} 项目服务<span v-if="serviceFor(project.project_id)?.port"> · 127.0.0.1:{{ serviceFor(project.project_id)?.port }}</span>
                  </p>
                  <Button class="h-8 w-full bg-blue-600 text-xs text-white hover:bg-blue-700" :disabled="!serviceFor(project.project_id)?.running || !project.demo_path" @click="emit('call', project.project_id)">
                    调用 {{ project.display_name }}
                    <ArrowRight class="size-3.5" />
                  </Button>
                </CardContent>
              </Card>
            </div>
            <p v-else class="rounded-lg border border-dashed border-zinc-200 bg-[#fafbfe] p-4 text-sm text-zinc-500">
              暂无已注册的业务项目。
            </p>
          </CardContent>
        </Card>

        <Card class="mt-4 bg-white/80 shadow-sm shadow-zinc-900/[0.03]">
          <CardHeader class="pb-3">
            <CardTitle class="text-base">
              最近一次调用
            </CardTitle>
            <CardDescription class="mt-1">
              用于快速确认业务服务可以被正常访问。
            </CardDescription>
          </CardHeader>
          <CardContent>
            <div class="flex min-h-20 items-start gap-3 rounded-lg border border-dashed border-zinc-200 bg-[#fafbfe] p-3">
              <CircleAlert class="mt-0.5 size-4 shrink-0 text-zinc-400" />
              <p class="text-sm leading-6 text-zinc-500">
                {{ props.message }}
              </p>
            </div>
            <p class="mt-3 flex items-center gap-1.5 text-xs text-zinc-400">
              <ExternalLink class="size-3" />
              后续项目可以复用这里的服务状态与调用入口。
            </p>
          </CardContent>
        </Card>
      </div>
    </ScrollArea>
  </main>
</template>
