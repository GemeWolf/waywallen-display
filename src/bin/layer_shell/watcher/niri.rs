use crate::watcher::{BindingRegistry, Command, CommandSender, OutputInfo};
use niri_ipc::socket::Socket;
use niri_ipc::state::{EventStreamState, EventStreamStatePart, WindowsState, WorkspacesState};
use niri_ipc::{Request, Response, Window};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use thiserror::Error;
use waywallen_display::{
    WAYWALLEN_WIN_HAS_ACTIVE, WAYWALLEN_WIN_HAS_FULLSCREEN, WAYWALLEN_WIN_HAS_NON_MINIMIZED,
};

#[derive(Error, Debug)]
pub enum Error {
    #[error("compositor response: {0}")]
    CompositorResponse(String),
    #[error("unexpected response: {0:?}")]
    UnexpectedResponse(Response),
}

pub fn detect_socket() -> Option<impl AsRef<Path>> {
    let niri_socket = std::env::var_os("NIRI_SOCKET")?;
    let path: &Path = niri_socket.as_ref();
    if path.exists() {
        Some(niri_socket)
    } else {
        None
    }
}

pub fn spawn(registry: BindingRegistry, commands: CommandSender) -> bool {
    let Some(sock) = detect_socket() else {
        return false;
    };
    log::info!("niri_watcher: enabled (socket={})", sock.as_ref().display());
    Socket::connect_to(sock.as_ref())
        .map(|mut event_socket| {
            event_socket
                .send(Request::EventStream)
                .map(|reply| match reply {
                    Ok(response) => match response {
                        Response::Handled => {
                            thread::spawn(move || run_loop(event_socket, registry, commands));
                            true
                        }
                        response => {
                            log::error!("niri_watcher: {}", Error::UnexpectedResponse(response));
                            false
                        }
                    },
                    Err(error) => {
                        log::error!("niri_watcher: {}", Error::CompositorResponse(error));
                        false
                    }
                })
                .unwrap_or_else(|error| {
                    log::error!("niri_watcher: request eventstream: {error}");
                    false
                })
        })
        .unwrap_or_else(|error| {
            log::error!("niri_watcher: connect {}: {error}", sock.as_ref().display());
            false
        })
}

fn run_loop(event_socket: Socket, registry: BindingRegistry, commands: CommandSender) {
    let mut state = EventStreamState::default();
    let mut read_event = event_socket.read_events();
    loop {
        let event = match read_event() {
            Ok(event) => event,
            Err(error) => {
                log::error!("niri_watcher: read event: {error}");
                break;
            }
        };
        state.apply(event);
        let snapshots =
            get_outputs_windows(&registry.lock().unwrap(), &state.workspaces, &state.windows);
        for (display_name, windows) in snapshots {
            commands.send(Command::WindowState {
                display_name,
                windows,
            });
        }
    }
}

fn get_outputs_windows(
    outputs: &HashMap<String, Arc<OutputInfo>>,
    workspaces_state: &WorkspacesState,
    windows_state: &WindowsState,
) -> Vec<(String, Vec<crate::watcher::Window>)> {
    let mut changed = Vec::new();
    for workspace in workspaces_state
        .workspaces
        .values()
        .filter(|workspace| workspace.is_active)
    {
        let Some(output_name) = workspace.output.as_ref() else {
            continue;
        };
        let size = outputs
            .get(output_name)
            .and_then(|output| output.logical_size())
            .map(|(w, h)| (w as i32, h as i32))
            .unwrap_or((0, 0));
        let windows = windows_state
            .windows
            .values()
            .filter(|window| window.workspace_id == Some(workspace.id))
            .map(|window| crate::watcher::Window {
                application_id: window.app_id.clone().unwrap_or_default(),
                title: window.title.clone().unwrap_or_default(),
                flags: window_to_flags(size, window),
            })
            .collect();
        changed.push((output_name.clone(), windows));
    }
    changed
}

// The IPC does not report fullscreen/maximized state; retain the geometry heuristic.
fn window_to_flags(fullscreen: (i32, i32), window: &Window) -> u32 {
    let mut flags = 0;
    flags |= WAYWALLEN_WIN_HAS_NON_MINIMIZED;
    if window.is_focused {
        flags |= WAYWALLEN_WIN_HAS_ACTIVE;
    }
    if is_window_fullscreen(fullscreen, window) {
        flags |= WAYWALLEN_WIN_HAS_FULLSCREEN
    } /* else if is_window_maximized(fullscreen, window) {
          flags |= WAYWALLEN_WIN_HAS_MAXIMIZED
      } */
    flags
}

// TODO: Waiting on https://github.com/niri-wm/niri/pull/2836 for proper logic
fn is_window_fullscreen(fullscreen: (i32, i32), window: &Window) -> bool {
    !window.is_floating && window.layout.window_size == fullscreen
}
