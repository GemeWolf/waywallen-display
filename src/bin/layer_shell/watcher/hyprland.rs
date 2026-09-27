use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::watcher::{BindingRegistry, Command as WatcherCommand, CommandSender, Window};
use waywallen_display::{
    WAYWALLEN_WIN_HAS_ACTIVE, WAYWALLEN_WIN_HAS_FULLSCREEN, WAYWALLEN_WIN_HAS_MAXIMIZED,
    WAYWALLEN_WIN_HAS_NON_MINIMIZED,
};

pub fn detect_socket() -> Option<PathBuf> {
    let his = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    let xdg = std::env::var_os("XDG_RUNTIME_DIR")?;
    let mut path = PathBuf::from(xdg);
    path.push("hypr");
    path.push(his);
    path.push(".socket2.sock");
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

pub fn spawn(registry: BindingRegistry, commands: CommandSender) -> bool {
    let Some(sock) = detect_socket() else {
        return false;
    };
    log::info!("hyprland_watcher: enabled (socket={})", sock.display());
    let stream = match UnixStream::connect(&sock) {
        Ok(stream) => stream,
        Err(error) => {
            log::warn!("hyprland_watcher: connect: {error}");
            return false;
        }
    };
    thread::spawn(move || run_loop(sock, stream, registry, commands));
    true
}

fn run_loop(
    socket_path: PathBuf,
    initial_stream: UnixStream,
    registry: BindingRegistry,
    commands: CommandSender,
) {
    let mut initial_stream = Some(initial_stream);
    loop {
        match initial_stream
            .take()
            .map(Ok)
            .unwrap_or_else(|| UnixStream::connect(&socket_path))
        {
            Ok(stream) => {
                push_state(&registry, &commands);
                let reader = BufReader::new(stream);
                for line in reader.lines() {
                    match line {
                        Ok(_) => push_state(&registry, &commands),
                        Err(e) => {
                            log::warn!("hyprland_watcher: read error: {e}");
                            break;
                        }
                    }
                }
            }
            Err(e) => log::warn!("hyprland_watcher: connect {}: {e}", socket_path.display()),
        }
        thread::sleep(Duration::from_secs(2));
    }
}

fn push_state(_registry: &BindingRegistry, commands: &CommandSender) {
    let snapshot = match hyprctl_snapshot() {
        Ok(v) => v,
        Err(e) => {
            log::warn!("hyprland_watcher: hyprctl: {e}");
            return;
        }
    };
    let by_output = windows_by_output(&snapshot);
    for monitor in &snapshot.monitors {
        commands.send(WatcherCommand::WindowState {
            display_name: monitor.name.clone(),
            windows: by_output.get(&monitor.name).cloned().unwrap_or_default(),
        });
    }
}

#[derive(serde::Deserialize)]
struct Client {
    #[serde(default)]
    class: String,
    #[serde(default)]
    title: String,
    address: String,
    monitor: i64,
    workspace: Workspace,
    fullscreen: i64,
    mapped: bool,
}

#[derive(serde::Deserialize)]
struct Workspace {
    id: i64,
}

#[derive(serde::Deserialize)]
struct Monitor {
    id: i64,
    name: String,
    #[serde(rename = "activeWorkspace")]
    active_workspace: WorkspaceRef,
}

#[derive(serde::Deserialize)]
struct WorkspaceRef {
    id: i64,
}

#[derive(serde::Deserialize)]
struct ActiveWindow {
    address: String,
}

struct Snapshot {
    clients: Vec<Client>,
    monitors: Vec<Monitor>,
    active_addr: Option<String>,
}

fn hyprctl_snapshot() -> anyhow::Result<Snapshot> {
    let clients = run_hyprctl_json::<Vec<Client>>(&["clients", "-j"])?;
    let monitors = run_hyprctl_json::<Vec<Monitor>>(&["monitors", "-j"])?;
    let active_addr = run_hyprctl_json::<ActiveWindow>(&["activewindow", "-j"])
        .ok()
        .map(|a| a.address)
        .filter(|s| !s.is_empty());
    Ok(Snapshot {
        clients,
        monitors,
        active_addr,
    })
}

fn run_hyprctl_json<T: serde::de::DeserializeOwned>(args: &[&str]) -> anyhow::Result<T> {
    let out = Command::new("hyprctl").args(args).output()?;
    if !out.status.success() {
        anyhow::bail!("hyprctl {args:?} exit {}", out.status);
    }
    Ok(serde_json::from_slice(&out.stdout)?)
}

fn windows_by_output(snap: &Snapshot) -> HashMap<String, Vec<Window>> {
    let mon_name: HashMap<i64, String> = snap
        .monitors
        .iter()
        .map(|m| (m.id, m.name.clone()))
        .collect();
    let active_ws: HashMap<i64, i64> = snap
        .monitors
        .iter()
        .map(|m| (m.id, m.active_workspace.id))
        .collect();
    let active = snap.active_addr.as_deref();
    let mut out: HashMap<String, Vec<Window>> = HashMap::new();
    for c in &snap.clients {
        if !c.mapped {
            continue;
        }
        let Some(name) = mon_name.get(&c.monitor) else {
            continue;
        };
        if active_ws.get(&c.monitor) != Some(&c.workspace.id) {
            continue;
        }
        let mut flags = WAYWALLEN_WIN_HAS_NON_MINIMIZED;
        if Some(c.address.as_str()) == active {
            flags |= WAYWALLEN_WIN_HAS_ACTIVE;
        }
        match c.fullscreen {
            1 => flags |= WAYWALLEN_WIN_HAS_MAXIMIZED,
            2 => flags |= WAYWALLEN_WIN_HAS_FULLSCREEN,
            _ => {}
        }
        out.entry(name.clone()).or_default().push(Window {
            application_id: c.class.clone(),
            title: c.title.clone(),
            flags,
        });
    }
    out
}
