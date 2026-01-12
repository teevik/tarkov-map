use eframe::egui;
use egui_toast::{Toast, ToastKind, ToastOptions, Toasts};
use std::sync::mpsc;
use std::thread;

const REPO_OWNER: &str = "teevik";
const REPO_NAME: &str = "tarkov-map";
const BIN_NAME: &str = "tarkov-map";

const UPDATE_AVAILABLE_TOAST_KIND: u32 = 1;
const RESTART_TOAST_KIND: u32 = 2;

#[derive(Debug, Clone, Copy)]
enum Command {
    UpdateNow,
    Restart,
}

#[derive(Debug)]
enum Event {
    UpdateAvailable { version: String },
    UpdateInstalled { version: String },
    UpToDate { version: String },
    CheckFailed { message: String },
    UpdateFailed { message: String },
}

/// Small helper that checks GitHub releases and can self-replace.
pub struct Updater {
    cmd_tx: mpsc::Sender<Command>,
    cmd_rx: mpsc::Receiver<Command>,
    event_tx: mpsc::Sender<Event>,
    event_rx: mpsc::Receiver<Event>,
    available_version: Option<String>,
    update_in_progress: bool,
}

impl Updater {
    pub fn new(ctx: egui::Context) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();

        spawn_update_check(ctx, event_tx.clone());

        Self {
            cmd_tx,
            cmd_rx,
            event_tx,
            event_rx,
            available_version: None,
            update_in_progress: false,
        }
    }

    /// Registers custom toast renderers for update and restart prompts.
    pub fn configure_toasts(&self, toasts: Toasts) -> Toasts {
        let cmd_tx = self.cmd_tx.clone();
        let toasts = toasts.custom_contents(UPDATE_AVAILABLE_TOAST_KIND, move |ui, toast| {
            render_action_toast(
                ui,
                toast,
                toast.style.info_icon.clone(),
                "Update",
                Command::UpdateNow,
                &cmd_tx,
            )
        });

        let cmd_tx = self.cmd_tx.clone();
        toasts.custom_contents(RESTART_TOAST_KIND, move |ui, toast| {
            render_action_toast(
                ui,
                toast,
                toast.style.success_icon.clone(),
                "Restart",
                Command::Restart,
                &cmd_tx,
            )
        })
    }

    pub fn poll(&mut self, ctx: &egui::Context, toasts: &mut Toasts) {
        self.poll_events(toasts);
        self.poll_commands(ctx, toasts);
    }

    fn poll_events(&mut self, toasts: &mut Toasts) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                Event::UpdateAvailable { version } => {
                    self.available_version = Some(version.clone());
                    toasts.add(Toast {
                        kind: ToastKind::Custom(UPDATE_AVAILABLE_TOAST_KIND),
                        text: format!("Update available: v{version}").into(),
                        options: ToastOptions::default().show_progress(false),
                        ..Default::default()
                    });
                }
                Event::UpdateInstalled { version } => {
                    self.update_in_progress = false;
                    toasts.add(Toast {
                        kind: ToastKind::Custom(RESTART_TOAST_KIND),
                        text: format!("Updated to v{version}. Restart to apply.").into(),
                        options: ToastOptions::default().show_progress(false),
                        ..Default::default()
                    });
                }
                Event::UpToDate { version } => {
                    self.update_in_progress = false;
                    toasts.add(Toast {
                        kind: ToastKind::Info,
                        text: format!("Already up to date (v{version})").into(),
                        options: ToastOptions::default().duration_in_seconds(6.0),
                        ..Default::default()
                    });
                }
                Event::CheckFailed { message } => {
                    log::warn!("Update check failed: {message}");
                }
                Event::UpdateFailed { message } => {
                    self.update_in_progress = false;
                    toasts.add(Toast {
                        kind: ToastKind::Error,
                        text: format!("Update failed: {message}").into(),
                        options: ToastOptions::default().duration_in_seconds(10.0),
                        ..Default::default()
                    });
                }
            }
        }
    }

    fn poll_commands(&mut self, ctx: &egui::Context, toasts: &mut Toasts) {
        while let Ok(cmd) = self.cmd_rx.try_recv() {
            match cmd {
                Command::UpdateNow => {
                    if self.update_in_progress {
                        continue;
                    }

                    self.update_in_progress = true;
                    toasts.add(Toast {
                        kind: ToastKind::Info,
                        text: "Downloading update…".into(),
                        options: ToastOptions::default().duration_in_seconds(8.0),
                        ..Default::default()
                    });

                    let target_version_tag = self
                        .available_version
                        .as_deref()
                        .map(|version| format!("v{version}"));

                    spawn_update_install(ctx.clone(), self.event_tx.clone(), target_version_tag);
                }
                Command::Restart => match restart_self() {
                    Ok(()) => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    Err(err) => {
                        toasts.add(Toast {
                            kind: ToastKind::Error,
                            text: format!("Restart failed: {err}").into(),
                            options: ToastOptions::default().duration_in_seconds(10.0),
                            ..Default::default()
                        });
                    }
                },
            }
        }
    }
}

fn spawn_update_check(ctx: egui::Context, event_tx: mpsc::Sender<Event>) {
    thread::spawn(move || {
        let current_version = env!("CARGO_PKG_VERSION");

        let updater = self_update::backends::github::Update::configure()
            .repo_owner(REPO_OWNER)
            .repo_name(REPO_NAME)
            .bin_name(BIN_NAME)
            .current_version(current_version)
            .no_confirm(true)
            .show_output(false)
            .build();

        let send = |event: Event| {
            let _ = event_tx.send(event);
        };

        match updater {
            Ok(updater) => {
                let target = updater.target();
                let identifier = updater.identifier();

                match updater.get_latest_releases(current_version) {
                    Ok(releases) => {
                        let releases = releases
                            .into_iter()
                            .filter(|release| {
                                release.asset_for(&target, identifier.as_deref()).is_some()
                            })
                            .collect::<Vec<_>>();

                        let selected_release = releases
                            .iter()
                            .find(|release| {
                                self_update::version::bump_is_compatible(
                                    current_version,
                                    &release.version,
                                )
                                .unwrap_or(false)
                            })
                            .or_else(|| releases.first());

                        if let Some(release) = selected_release {
                            send(Event::UpdateAvailable {
                                version: release.version.clone(),
                            });
                        }
                    }
                    Err(err) => send(Event::CheckFailed {
                        message: err.to_string(),
                    }),
                }
            }
            Err(err) => send(Event::CheckFailed {
                message: err.to_string(),
            }),
        }

        ctx.request_repaint();
    });
}

fn spawn_update_install(
    ctx: egui::Context,
    event_tx: mpsc::Sender<Event>,
    target_version_tag: Option<String>,
) {
    thread::spawn(move || {
        let current_version = env!("CARGO_PKG_VERSION");

        let mut builder = self_update::backends::github::Update::configure();
        builder
            .repo_owner(REPO_OWNER)
            .repo_name(REPO_NAME)
            .bin_name(BIN_NAME)
            .current_version(current_version)
            .no_confirm(true)
            .show_output(false);

        if let Some(tag) = target_version_tag.as_deref() {
            builder.target_version_tag(tag);
        }

        let updater = builder.build();

        let send = |event: Event| {
            let _ = event_tx.send(event);
        };

        match updater {
            Ok(updater) => match updater.update() {
                Ok(status) => {
                    if status.uptodate() {
                        send(Event::UpToDate {
                            version: status.version().to_owned(),
                        });
                    } else {
                        send(Event::UpdateInstalled {
                            version: status.version().to_owned(),
                        });
                    }
                }
                Err(err) => send(Event::UpdateFailed {
                    message: err.to_string(),
                }),
            },
            Err(err) => send(Event::UpdateFailed {
                message: err.to_string(),
            }),
        }

        ctx.request_repaint();
    });
}

fn restart_self() -> std::io::Result<()> {
    let exe = std::env::current_exe()?;

    let mut cmd = std::process::Command::new(exe);
    cmd.args(std::env::args().skip(1));

    cmd.spawn()?;
    Ok(())
}

fn render_action_toast(
    ui: &mut egui::Ui,
    toast: &mut Toast,
    icon: egui::WidgetText,
    action_label: &str,
    action: Command,
    cmd_tx: &mpsc::Sender<Command>,
) -> egui::Response {
    let inner_margin = 10.0;
    let frame = egui::Frame::window(ui.style());

    let show_icon = toast.options.show_icon;
    let toast_text = toast.text.clone();
    let close_button_text = toast.style.close_button_text.clone();

    let mut action_clicked = false;
    let mut close_clicked = false;

    let response = frame
        .inner_margin(inner_margin)
        .stroke(egui::Stroke::NONE)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let is_rtl = ui.layout().prefer_right_to_left();

                if is_rtl {
                    close_clicked |= ui.button(close_button_text.clone()).clicked();
                    action_clicked |= ui.button(action_label).clicked();
                    ui.label(toast_text.clone());
                    if show_icon {
                        ui.label(icon.clone());
                    }
                } else {
                    if show_icon {
                        ui.label(icon.clone());
                    }
                    ui.label(toast_text.clone());
                    action_clicked |= ui.button(action_label).clicked();
                    close_clicked |= ui.button(close_button_text.clone()).clicked();
                }
            })
        })
        .response;

    if action_clicked {
        let _ = cmd_tx.send(action);
        toast.close();
    } else if close_clicked {
        toast.close();
    }

    // Draw the frame's stroke last (to match egui-toast default look)
    let frame_shape = egui::Shape::Rect(egui::epaint::RectShape::stroke(
        response.rect,
        frame.corner_radius,
        ui.visuals().window_stroke,
        egui::StrokeKind::Inside,
    ));
    ui.painter().add(frame_shape);

    response
}
