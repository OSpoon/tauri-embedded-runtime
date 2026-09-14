import { defineConfig } from "bumpp"

export default defineConfig({
  files: [
    "package.json",
    "src-tauri/Cargo.toml",
    "src-tauri/tauri.conf.json",
  ],
  commit: "release: v%s",
  tag: "v%s",
  push: true,
  install: true,
  all: true,
})
