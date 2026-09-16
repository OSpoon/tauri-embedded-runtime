import { createRouter, createWebHashHistory } from "vue-router"
import RuntimeWorkspace from "@/views/RuntimeWorkspace.vue"

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: "/",
      name: "runtime-workspace",
      component: RuntimeWorkspace,
    },
  ],
})
