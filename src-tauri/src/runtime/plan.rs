use super::projects::PROJECT_SERVICES;
use super::types::RuntimeSetupStep;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ProgressRange {
    pub(crate) start: u8,
    pub(crate) end: u8,
}

impl ProgressRange {
    pub(crate) const fn new(start: u8, end: u8) -> Self {
        Self { start, end }
    }

    pub(crate) fn at(self, percent: u8) -> u8 {
        let percent = percent.min(100) as u16;
        let width = u16::from(self.end.saturating_sub(self.start));
        self.start.saturating_add(((width * percent) / 100) as u8)
    }

    pub(crate) fn subrange(self, start_percent: u8, end_percent: u8) -> Self {
        Self::new(
            self.at(start_percent),
            self.at(end_percent.max(start_percent)),
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum BootstrapStageKind {
    Check,
    Runtime(&'static str),
    Projects(&'static str),
    Tools,
    Verify,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct BootstrapStage {
    pub(crate) id: &'static str,
    pub(crate) phase: &'static str,
    pub(crate) kind: BootstrapStageKind,
    pub(crate) progress: ProgressRange,
}

fn range_for(index: usize, total: usize) -> ProgressRange {
    let start = ((index * 100) / total) as u8;
    let end = (((index + 1) * 100) / total) as u8;
    ProgressRange::new(start, end.max(start.saturating_add(1)))
}

pub(crate) fn bootstrap_plan() -> Vec<BootstrapStage> {
    let total = PROJECT_SERVICES.len() * 2 + 3;
    let mut index = 0;
    let mut stages = Vec::with_capacity(total);

    stages.push(BootstrapStage {
        id: "check",
        phase: "check",
        kind: BootstrapStageKind::Check,
        progress: range_for(index, total),
    });
    index += 1;

    for service in PROJECT_SERVICES {
        stages.push(BootstrapStage {
            id: service.id,
            phase: service.id,
            kind: BootstrapStageKind::Runtime(service.id),
            progress: range_for(index, total),
        });
        index += 1;
        stages.push(BootstrapStage {
            id: service.project_step_id,
            phase: service.project_step_id,
            kind: BootstrapStageKind::Projects(service.id),
            progress: range_for(index, total),
        });
        index += 1;
    }

    stages.push(BootstrapStage {
        id: "project-tools",
        phase: "project-tools",
        kind: BootstrapStageKind::Tools,
        progress: range_for(index, total),
    });
    index += 1;
    stages.push(BootstrapStage {
        id: "verify",
        phase: "verify",
        kind: BootstrapStageKind::Verify,
        progress: range_for(index, total),
    });
    stages
}

pub(crate) fn setup_steps() -> Vec<RuntimeSetupStep> {
    bootstrap_plan()
        .into_iter()
        .map(|stage| {
            let (title, description) = match stage.kind {
                BootstrapStageKind::Check => (
                    "检查安装环境".to_string(),
                    "读取 manifest、锁和平台信息".to_string(),
                ),
                BootstrapStageKind::Runtime(service) => {
                    let spec = super::projects::service_for_id(service)
                        .expect("bootstrap plan references a registered service");
                    (
                        spec.runtime_title.to_string(),
                        spec.runtime_description.to_string(),
                    )
                }
                BootstrapStageKind::Projects(service) => {
                    let spec = super::projects::service_for_id(service)
                        .expect("bootstrap plan references a registered service");
                    (
                        spec.project_title.to_string(),
                        spec.project_description.to_string(),
                    )
                }
                BootstrapStageKind::Tools => (
                    "检查项目工具".to_string(),
                    "检查 ffmpeg 等项目级工具制品".to_string(),
                ),
                BootstrapStageKind::Verify => (
                    "验证运行时".to_string(),
                    "探测解释器并启动服务，检查健康接口".to_string(),
                ),
            };
            RuntimeSetupStep {
                id: stage.id.to_string(),
                phase: stage.phase.to_string(),
                title,
                description,
            }
        })
        .collect()
}
