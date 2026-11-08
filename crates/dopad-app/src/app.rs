use std::{ffi::OsString, path::PathBuf};

use dopad_ipc::{CliCommand, ResponseStatus};
use gpui_kit::{App, AppContext, application, init};

use crate::{
    cli_ipc::{self, UiMessage},
    logging,
    session::SessionCoordinator,
    settings::SettingsService,
    window,
};

pub fn run() {
    logging::initialize();
    let launch_paths = collect_launch_paths(std::env::args_os().skip(1));
    match cli_ipc::try_forward_launch(&launch_paths) {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => {
            // Let the exclusive IPC bind arbitrate after a transient forwarding
            // failure. If another instance owns the endpoint, the fallback below
            // retries delivery through that instance.
            tracing::warn!(error = %error, "initial launch forwarding failed; retrying through instance arbitration");
        }
    }
    let (ipc_sender, ipc_receiver) = async_channel::bounded(64);
    let ipc_server = match cli_ipc::start_server(ipc_sender) {
        Ok(server) => server,
        Err(error) => {
            tracing::debug!(
                endpoint = %dopad_ipc::endpoint_display(),
                error = %error,
                "IPC bind unavailable; retrying launch delivery to the active Desktop"
            );
            let forwarded = cli_ipc::forward_launch(&launch_paths);
            match forwarded {
                Ok(()) => return,
                Err(forward_error) => {
                    tracing::error!(
                        bind_error = %error,
                        forwarding_error = %forward_error,
                        "could not route the launch request to the active Desktop instance"
                    );
                    eprintln!("Dopad could not route the requested file: {forward_error}");
                    return;
                }
            }
        }
    };
    let session = SessionCoordinator::new();
    let app_session = session.clone();
    let settings = SettingsService::load();
    if let Err(error) = logging::set_level(&settings.current().logging_level) {
        tracing::warn!(error = %error, "could not apply configured logging level");
    }
    let app_settings = settings.clone();

    application()
        .with_assets(crate::icon_theme::DopadAssets)
        .run(move |cx: &mut App| {
            init(cx);
            crate::theme::apply(false, cx);
            let updater = cx.new(|_| crate::updater::UpdateManager::default());

            let main_window = match window::open_main(
                cx,
                app_session.clone(),
                app_settings.clone(),
                updater.clone(),
                launch_paths.clone(),
            ) {
                Ok(main_window) => main_window,
                Err(error) => {
                    tracing::error!(error = %error, "failed to open the Dopad window");
                    eprintln!("Dopad could not open its main window: {error:#}");
                    cx.quit();
                    return;
                }
            };
            if app_settings.current().auto_check_updates {
                updater.update(cx, |manager, cx| manager.check(cx, false));
            }
            let app_updater = updater.clone();
            cx.spawn(async move |cx| {
                let mut windows = vec![main_window];
                tracing::info!("main window ready for external IPC requests");
                while let Ok(message) = ipc_receiver.recv().await {
                    match message {
                        UiMessage::Probe {
                            request_id,
                            response,
                        } => {
                            let _ = response.send(cli_ipc::ready_response(request_id));
                        }
                        UiMessage::Request { request, response } => {
                            let request_id = request.request_id;
                            windows.retain(|main_window| {
                                main_window.handle.update(cx, |_, _, _| ()).is_ok()
                            });
                            let create_window = matches!(
                                &request.command,
                                CliCommand::Open {
                                    new_window: true,
                                    ..
                                }
                            );
                            let target_index = if create_window {
                                match cx.update(|cx| {
                                    window::open_main(
                                        cx,
                                        SessionCoordinator::ephemeral(),
                                        app_settings.clone(),
                                        app_updater.clone(),
                                        Vec::new(),
                                    )
                                }) {
                                    Ok(main_window) => {
                                        windows.push(main_window);
                                        Some(windows.len() - 1)
                                    }
                                    Err(error) => {
                                        let _ = response.send(cli_ipc::complete_response(
                                            request.request_id,
                                            ResponseStatus::InternalError,
                                            Some(format!(
                                                "could not open a new editor window: {error:#}"
                                            )),
                                        ));
                                        None
                                    }
                                }
                            } else {
                                let mut selected = None;
                                for index in (0..windows.len()).rev() {
                                    if windows[index]
                                        .handle
                                        .update(cx, |_, window, _| window.activate_window())
                                        .is_ok()
                                    {
                                        selected = Some(index);
                                        break;
                                    }
                                }
                                if selected.is_none() {
                                    match cx.update(|cx| {
                                        window::open_main(
                                            cx,
                                            SessionCoordinator::ephemeral(),
                                            app_settings.clone(),
                                            app_updater.clone(),
                                            Vec::new(),
                                        )
                                    }) {
                                        Ok(main_window) => {
                                            windows.push(main_window);
                                            selected = Some(windows.len() - 1);
                                        }
                                        Err(error) => {
                                            let _ = response.send(cli_ipc::complete_response(
                                                request.request_id,
                                                ResponseStatus::InternalError,
                                                Some(format!(
                                                    "could not open an editor window: {error:#}"
                                                )),
                                            ));
                                        }
                                    }
                                }
                                selected
                            };
                            if let Some(index) = target_index {
                                tracing::debug!(
                                    request_id,
                                    "dispatching CLI request to editor window"
                                );
                                windows[index].view.update(cx, |view, cx| {
                                    view.handle_cli_request(request, response, cx)
                                });
                            }
                        }
                        UiMessage::CancelWait { request_id } => {
                            for main_window in &windows {
                                main_window
                                    .view
                                    .update(cx, |view, _| view.cancel_cli_request(request_id));
                            }
                        }
                    }
                }
            })
            .detach();
        });

    ipc_server.shutdown();
    if let Err(error) = session.mark_clean_shutdown() {
        tracing::warn!(error = %error, "failed to mark the session as cleanly shut down");
    }
}

fn collect_launch_paths(args: impl IntoIterator<Item = OsString>) -> Vec<PathBuf> {
    args.into_iter().map(PathBuf::from).collect()
}

#[cfg(test)]
mod tests {
    use super::collect_launch_paths;
    use std::{ffi::OsString, path::PathBuf};

    #[test]
    fn preserves_file_manager_paths_with_spaces_and_unicode() {
        let path = r"C:\Users\Duong\Desktop\Bản nháp text.txt";
        assert_eq!(
            collect_launch_paths([OsString::from(path)]),
            vec![PathBuf::from(path)]
        );
    }
}
