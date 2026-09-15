use std::sync::{Mutex, MutexGuard};

use tauri::menu::MenuEvent;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::UpdaterExt;

use crate::menus::CHECK_UPDATES_MENU_ID;

#[derive(Default)]
pub struct UpdateState {
    running: Mutex<bool>,
}

impl UpdateState {
    fn lock(&self) -> MutexGuard<'_, bool> {
        self.running
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn try_start(&self) -> bool {
        let mut running = self.lock();
        if *running {
            return false;
        }
        *running = true;
        true
    }

    fn finish(&self) {
        *self.lock() = false;
    }
}

pub fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    if event.id().as_ref() != CHECK_UPDATES_MENU_ID {
        return;
    }

    let state = app.state::<UpdateState>();
    if !state.try_start() {
        show_message(
            app,
            "正在更新",
            "更新任务已经在运行，请稍候。",
            MessageDialogKind::Info,
        );
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        check_and_prompt(app).await;
    });
}

async fn check_and_prompt<R: Runtime>(app: AppHandle<R>) {
    let updater = match app.updater() {
        Ok(updater) => updater,
        Err(error) => {
            finish(&app);
            show_error(&app, "无法初始化更新服务", error);
            return;
        }
    };

    match updater.check().await {
        Ok(Some(update)) => {
            let version = update.version.clone();
            let notes = update
                .body
                .clone()
                .unwrap_or_else(|| "此版本没有提供更新说明。".to_string());
            let app_for_install = app.clone();

            app.dialog()
                .message(format!(
                    "发现新版本 v{version}。\n\n{notes}\n\n是否现在下载并安装？",
                ))
                .title("发现更新")
                .kind(MessageDialogKind::Info)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    "下载并安装".to_string(),
                    "稍后".to_string(),
                ))
                .show(move |confirmed| {
                    if !confirmed {
                        finish(&app_for_install);
                        return;
                    }

                    tauri::async_runtime::spawn(async move {
                        install_update(app_for_install, update).await;
                    });
                });
        }
        Ok(None) => {
            finish(&app);
            let version = app.package_info().version.to_string();
            show_message(
                &app,
                "已是最新版本",
                format!("当前版本 v{version} 已是最新版本。"),
                MessageDialogKind::Info,
            );
        }
        Err(error) => {
            finish(&app);
            show_error(&app, "检查更新失败", error);
        }
    }
}

async fn install_update<R: Runtime>(app: AppHandle<R>, update: tauri_plugin_updater::Update) {
    let result = update.download_and_install(|_, _| {}, || {}).await;

    match result {
        Ok(()) => {
            finish(&app);
            app.restart();
        }
        Err(error) => {
            finish(&app);
            show_error(&app, "安装更新失败", error);
        }
    }
}

fn finish<R: Runtime>(app: &AppHandle<R>) {
    app.state::<UpdateState>().finish();
}

fn show_message<R: Runtime>(
    app: &AppHandle<R>,
    title: &str,
    message: impl Into<String>,
    kind: MessageDialogKind,
) {
    app.dialog()
        .message(message)
        .title(title)
        .kind(kind)
        .buttons(MessageDialogButtons::Ok)
        .show(|_| {});
}

fn show_error<R: Runtime>(app: &AppHandle<R>, title: &str, error: impl std::fmt::Display) {
    show_message(app, title, error.to_string(), MessageDialogKind::Error);
}
