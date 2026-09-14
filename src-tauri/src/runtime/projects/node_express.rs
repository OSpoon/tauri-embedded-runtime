use super::ProjectModuleSpec;

pub(crate) const SERVICE: super::ProjectServiceSpec = super::ProjectServiceSpec {
    id: "node",
    display_name: "Node.js 服务",
    runtime_title: "准备 Node.js",
    runtime_description: "下载并校验私有 Node.js 运行时",
    project_step_id: "project-node",
    project_title: "安装 Express 环境",
    project_description: "安装独立 node_modules 并生成 package-lock",
};

pub(crate) const MODULE: ProjectModuleSpec = ProjectModuleSpec {
    project_id: "node-express",
    service: &SERVICE,
    profile_source: include_str!("../../../resources/projects/node-express/project.json"),
    service_source: include_str!("../../../resources/projects/node-express/service.mjs"),
};
