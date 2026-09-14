use super::ProjectModuleSpec;

pub(crate) const SERVICE: super::ProjectServiceSpec = super::ProjectServiceSpec {
    id: "python",
    display_name: "Python 服务",
    runtime_title: "准备 Python",
    runtime_description: "下载并校验私有 CPython 运行时",
    project_step_id: "project-python",
    project_title: "安装 FastAPI 环境",
    project_description: "创建独立 venv 并安装 Python 项目依赖",
};

pub(crate) const MODULE: ProjectModuleSpec = ProjectModuleSpec {
    project_id: "python-fastapi",
    service: &SERVICE,
    profile_source: include_str!("../../../resources/projects/python-fastapi/project.json"),
    service_source: include_str!("../../../resources/projects/python-fastapi/service.py"),
};
