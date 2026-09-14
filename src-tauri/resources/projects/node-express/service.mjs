import express from "express"

const portIndex = process.argv.indexOf("--port")
const port = Number(process.argv[portIndex + 1])
const app = express()

app.use((request, response, next) => {
  response.setHeader("Access-Control-Allow-Origin", "*")
  response.setHeader("Access-Control-Allow-Methods", "GET, OPTIONS")
  response.setHeader("Access-Control-Allow-Headers", "Content-Type")
  if (request.method === "OPTIONS") {
    response.sendStatus(204)
    return
  }
  next()
})

app.get("/health", (_request, response) => {
  response.json({ ok: true, runtime: "node", framework: "express" })
})

app.get("/api/hello", (request, response) => {
  response.json({
    ok: true,
    runtime: "node",
    framework: "express",
    message: `Hello, ${request.query.name ?? "developer"}, from Express!`,
  })
})

app.listen(port, "127.0.0.1", () => {
  process.stdout.write(`NODE_PROJECT_READY ${port}\n`)
})
