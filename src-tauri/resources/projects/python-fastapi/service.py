"""FastAPI project environment smoke-test service."""

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
import uvicorn

app = FastAPI(title="Runtime Python Demo", version="1.0")
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=False,
    allow_methods=["GET"],
    allow_headers=["*"],
)


@app.get("/health")
def health() -> dict[str, object]:
    return {"ok": True, "runtime": "python", "framework": "fastapi"}


@app.get("/api/hello")
def hello(name: str = "developer") -> dict[str, object]:
    return {"ok": True, "runtime": "python", "framework": "fastapi", "message": f"Hello, {name}, from FastAPI!"}


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, required=True)
    args = parser.parse_args()
    uvicorn.run(app, host="127.0.0.1", port=args.port, log_level="info")
