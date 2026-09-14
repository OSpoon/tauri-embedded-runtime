//! Bundled project modules.
//!
//! Each service owns its profile and entrypoint source. The runtime lifecycle
//! consumes this registry without knowing anything about a particular
//! framework, so real projects can be added without changing installation,
//! probing, or supervision logic.

mod node_express;
mod python_fastapi;

pub(crate) struct ProjectModuleSpec {
    pub(crate) project_id: &'static str,
    pub(crate) service: &'static ProjectServiceSpec,
    pub(crate) profile_source: &'static str,
    pub(crate) service_source: &'static str,
}

pub(crate) struct ProjectServiceSpec {
    pub(crate) id: &'static str,
    pub(crate) display_name: &'static str,
    pub(crate) runtime_title: &'static str,
    pub(crate) runtime_description: &'static str,
    pub(crate) project_step_id: &'static str,
    pub(crate) project_title: &'static str,
    pub(crate) project_description: &'static str,
}

pub(crate) const PROJECT_SERVICES: &[&ProjectServiceSpec] =
    &[&python_fastapi::SERVICE, &node_express::SERVICE];

pub(crate) const PROJECT_MODULES: &[ProjectModuleSpec] =
    &[python_fastapi::MODULE, node_express::MODULE];

pub(crate) fn service_for_id(service: &str) -> Option<&'static ProjectServiceSpec> {
    PROJECT_SERVICES
        .iter()
        .copied()
        .find(|spec| spec.id == service)
}
