# 项目接入指南

本文面向需要把真实 Python 或 Node.js 服务接入运行时管理器的开发者。

## 接入原则

每个业务项目使用独立目录和独立 generation。项目只声明自己的依赖、入口和健康检查
信息；运行时安装、依赖隔离、版本校验、取消、恢复和服务进程管理由公共运行时层负责。

同一个运行时类型可以注册多个项目。项目会自动归入对应的“准备运行时”和“安装项目
环境”阶段，不需要为每个项目复制安装流程。

## 接入一个 Python 或 Node.js 项目

### 1. 创建项目资源目录

目录名使用稳定的项目 ID：

```text
src-tauri/resources/projects/<project-id>/
├── project.json
└── service.py            # Python 项目
    或 service.mjs        # Node.js 项目
```

不同服务禁止共用项目目录，也不要把多个项目的依赖文件放在同一个目录。

### 2. 编写项目清单

Python 项目示例：

```json
{
  "schema_version": 1,
  "project_id": "python-fastapi",
  "service": "python",
  "framework": "fastapi",
  "display_name": "FastAPI",
  "entrypoint": "services/python_service.py",
  "launch_args": ["{entrypoint}", "--port", "{port}"],
  "health_path": "/health",
  "demo_path": "/api/hello?name=developer",
  "requirements": ["fastapi==0.115.6", "uvicorn==0.34.0"],
  "tools": []
}
```

Node.js 项目使用 `dependencies`，其余启动字段相同：

```json
{
  "schema_version": 1,
  "project_id": "node-express",
  "service": "node",
  "framework": "express",
  "display_name": "Express",
  "entrypoint": "services/node_service.mjs",
  "launch_args": ["{entrypoint}", "--port", "{port}"],
  "health_path": "/health",
  "demo_path": "/api/hello?name=developer",
  "package_name": "runtime-node-demo",
  "dependencies": { "express": "4.21.2" },
  "tools": []
}
```

字段说明：

| 字段 | 说明 |
| --- | --- |
| `project_id` | 全局唯一，只允许安全的目录名称 |
| `service` | 当前支持 `python` 或 `node` |
| `entrypoint` | 相对于项目 generation 的入口路径 |
| `launch_args` | 启动参数，支持 `{entrypoint}` 和 `{port}` |
| `health_path` | 服务健康检查路径 |
| `demo_path` | 可选，业务首页的示例调用路径 |
| `requirements` | Python 锁定依赖版本 |
| `dependencies` | Node.js 锁定依赖版本 |
| `tools` | 项目级工具；`required: true` 时必须准备成功 |

### 3. 注册项目模块

在 `src-tauri/src/runtime/projects/` 新增项目模块描述，并使用 `include_str!` 引入
项目清单和入口源码：

```rust
use super::{ProjectModuleSpec, python_fastapi::SERVICE};

pub(crate) const MODULE: ProjectModuleSpec = ProjectModuleSpec {
    project_id: "python-real-project",
    service: &SERVICE,
    profile_source: include_str!(
        "../../../resources/projects/python-real-project/project.json"
    ),
    service_source: include_str!(
        "../../../resources/projects/python-real-project/service.py"
    ),
};
```

然后在 `projects/mod.rs` 中完成两处注册：

```rust
mod python_real_project;

pub(crate) const PROJECT_MODULES: &[ProjectModuleSpec] = &[
    python_fastapi::MODULE,
    python_real_project::MODULE,
    node_express::MODULE,
];
```

如果项目属于已有 Python 或 Node.js 服务，只需要复用已有的 `SERVICE`。服务注册顺序
只在新增运行时服务时调整；同一服务下新增项目不会改变向导阶段顺序。

### 4. 编写服务入口

入口文件必须：

- 只使用项目清单中声明的依赖
- 从 `{port}` 对应的参数启动
- 提供 `health_path` 对应的 HTTP 200 接口
- 不自行修改宿主机 PATH、全局包目录或用户配置
- 能响应应用的取消和关闭流程

### 5. 验证接入

```bash
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
```

启动应用后点击“运行检测”，确认项目依赖阶段、项目工具阶段和运行时验证阶段均能
完成，并在业务首页看到自动生成的项目卡片。

## 哪些内容不需要修改

接入已有 Python 或 Node.js 服务类型时，不需要修改：

- `runtime/commands.rs`
- `runtime/plan.rs`
- `runtime/install.rs`
- `runtime/project.rs`
- `RuntimeHome.vue`
- `useRuntimeSetup.ts`

向导步骤和业务首页会通过运行时计划、项目目录清单和项目目录元数据自动更新。

## 新增全新运行时类型

如果要接入 Go、Java、Deno 等新的基础运行时，目前还不是纯配置扩展，需要增加对应
的运行时适配实现，包括制品选择、版本探测、generation 清单、项目依赖安装和服务
启动方式。建议先沿用现有的 generation、事务、锁、日志和进程监督机制，再实现新的
适配器，不要在项目模块中直接调用宿主机工具链。
