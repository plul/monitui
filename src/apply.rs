use crate::{hyprland, monitor::MonitorInfo};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn monitors_lua_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("hypr")
        .join("monitors.lua")
}

fn monitor_lua(monitor: &MonitorInfo) -> String {
    let output = hyprland::lua_string(&monitor.name);
    if monitor.disabled {
        format!("hl.monitor({{ output = {}, disabled = true }})", output)
    } else {
        format!(
            "hl.monitor({{ output = {}, mode = {}, position = {}, scale = {}, transform = {}, disabled = false }})",
            output,
            hyprland::lua_string(&monitor.mode_string()),
            hyprland::lua_string(&format!("{}x{}", monitor.x, monitor.y)),
            format_scale(monitor.scale),
            monitor.transform
        )
    }
}

/// Generate a Lua fragment to load after the user's other monitor rules.
fn generate_monitors_lua(monitors: &[MonitorInfo]) -> String {
    let mut lines = Vec::new();
    lines.push("-- Managed by monitui — https://github.com/nathanielbd/monitui".to_string());
    lines.push("-- Manual edits will be overwritten on next apply.".to_string());
    lines.push(
        "-- Disabled monitors are not persisted; they are applied at runtime only.".to_string(),
    );
    lines.push(String::new());

    for m in monitors {
        if m.disabled {
            continue;
        }
        lines.push(monitor_lua(m));
    }

    lines.push(String::new());
    lines.join("\n")
}

/// Apply Lua monitor rules and persist enabled outputs in monitors.lua.
pub fn apply_monitors(monitors: &[MonitorInfo]) -> Result<(), String> {
    let conf_path = monitors_lua_path();
    let content = generate_monitors_lua(monitors);
    if let Some(parent) = conf_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }
    fs::write(&conf_path, &content)
        .map_err(|e| format!("Failed to write {}: {}", conf_path.display(), e))?;

    // Reload Hyprland configuration so file-backed state is active first.
    hyprland::reload()?;

    // Enable outputs before disabling others, including when a preset swaps them.
    let lua = monitors
        .iter()
        .filter(|m| !m.disabled)
        .chain(monitors.iter().filter(|m| m.disabled))
        .map(monitor_lua)
        .collect::<Vec<_>>()
        .join("\n");
    if !lua.is_empty() {
        hyprland::eval(&lua)?;
    }

    for monitor in monitors.iter().filter(|m| !m.disabled) {
        for ws in &monitor.workspaces {
            hyprland::move_workspace(*ws, &monitor.name)?;
        }
    }

    Command::new("notify-send")
        .args(["monitui", "Monitor configuration applied"])
        .output()
        .ok();

    Ok(())
}

fn format_scale(scale: f32) -> String {
    if (scale - scale.round()).abs() < 0.001 {
        format!("{}", scale as u32)
    } else {
        format!("{:.6}", scale)
    }
}

#[cfg(test)]
mod tests {
    use super::{generate_monitors_lua, monitor_lua};
    use crate::monitor::MonitorInfo;

    fn test_monitor(name: &str, disabled: bool) -> MonitorInfo {
        MonitorInfo {
            name: name.to_string(),
            description: "Test monitor".to_string(),
            width: 1920,
            height: 1080,
            refresh_rate: 60.0,
            x: 0,
            y: 0,
            scale: 1.0,
            disabled,
            transform: 0,
            workspaces: vec![],
            available_modes: vec![],
            selected_mode: None,
        }
    }

    #[test]
    fn monitors_lua_excludes_disabled_monitors() {
        let monitors = vec![test_monitor("DP-1", false), test_monitor("HDMI-A-1", true)];

        let content = generate_monitors_lua(&monitors);

        assert!(content.contains("hl.monitor({ output = \"DP-1\", mode = \"preferred\", position = \"0x0\", scale = 1, transform = 0, disabled = false })"));
        assert!(!content.contains("HDMI-A-1"));
    }

    #[test]
    fn lua_preserves_monitor_settings_and_escapes_output_names() {
        let mut monitor = test_monitor("desc:Écran \"TV\"\\", false);
        monitor.selected_mode = Some(0);
        monitor.refresh_rate = 59.95;
        monitor.x = -1536;
        monitor.y = 100;
        monitor.scale = 1.25;
        monitor.transform = 3;
        let lua = monitor_lua(&monitor);
        assert!(lua.contains("output = \"desc:Écran \\\"TV\\\"\\\\\""));
        assert!(lua.contains("mode = \"1920x1080@59.95\""));
        assert!(lua.contains("position = \"-1536x100\""));
        assert!(lua.contains("scale = 1.250000, transform = 3, disabled = false"));
    }

    #[test]
    fn runtime_disable_uses_a_lua_boolean() {
        assert_eq!(
            monitor_lua(&test_monitor("DP-1", true)),
            "hl.monitor({ output = \"DP-1\", disabled = true })"
        );
    }
}
