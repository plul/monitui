use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct Session {
    root: PathBuf,
}

impl Session {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "monitui-lua-test-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("bin")).unwrap();
        let session = Self { root };
        session.script(
            "hyprctl",
            r#"#!/bin/sh
printf '%s\n' "$@" >> "$MONITUI_TEST_ROOT/commands"
case "$1" in
    -j) cat "$MONITUI_TEST_ROOT/monitors.json" ;;
    reload) printf 'ok\n' ;;
    eval)
        if test -f "$MONITUI_TEST_ROOT/fail-eval"; then
            printf 'Lua error: invalid monitor spec\n'
        else
            printf 'ok\n'
        fi ;;
    *) printf 'Unexpected legacy command\n'; exit 1 ;;
esac
"#,
        );
        session.script("notify-send", "#!/bin/sh\nexit 0\n");
        fs::write(
            session.root.join("monitors.json"),
            json!([
                { "name": "DP-1", "width": 1920, "height": 1080,
                  "refreshRate": 59.95, "scale": 1, "disabled": false,
                  "activeWorkspace": { "id": 1 },
                  "availableModes": ["1920x1080@59.95Hz"] },
                { "name": "HDMI-A-1", "width": 1920, "height": 1080,
                  "refreshRate": 60, "scale": 1, "disabled": true }
            ])
            .to_string(),
        )
        .unwrap();
        session
    }

    fn script(&self, name: &str, content: &str) {
        let path = self.root.join("bin").join(name);
        fs::write(&path, content).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        let mut paths = vec![self.root.join("bin")];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        Command::new(env!("CARGO_BIN_EXE_monitui"))
            .args(args)
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("MONITUI_TEST_ROOT", &self.root)
            .output()
            .unwrap()
    }

    fn commands(&self) -> String {
        fs::read_to_string(self.root.join("commands")).unwrap()
    }

    fn saved_lua(&self) -> String {
        fs::read_to_string(self.root.join("config/hypr/monitors.lua")).unwrap()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn enable_and_disable_use_lua_and_preserve_the_legacy_file() {
    let session = Session::new();
    let legacy = session.root.join("config/hypr/monitors.conf");
    fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    fs::write(&legacy, "legacy configuration\n").unwrap();
    let enabled = session.run(&["--enable", "HDMI-A-1"]);
    assert!(enabled.status.success(), "{:?}", enabled);
    assert!(session.saved_lua().contains("output = \"HDMI-A-1\""));
    assert!(session
        .commands()
        .contains("hl.dispatch(hl.dsp.workspace.move({ workspace = \"1\", monitor = \"DP-1\" }))"));

    let disabled = session.run(&["--disable", "DP-1"]);
    assert!(disabled.status.success(), "{:?}", disabled);
    assert!(!session.saved_lua().contains("hl.monitor("));
    assert!(session
        .commands()
        .contains("hl.monitor({ output = \"DP-1\", disabled = true })"));
    assert_eq!(
        fs::read_to_string(legacy).unwrap(),
        "legacy configuration\n"
    );
    assert!(!session.commands().contains("keyword"));
}

#[test]
fn existing_json_presets_and_recent_layouts_apply_through_lua() {
    let session = Session::new();
    let presets = session.root.join("config/monitui/presets");
    fs::create_dir_all(&presets).unwrap();
    fs::write(
        presets.join("desk.json"),
        json!({
            "name": "desk", "monitors": [{
                "name": "DP-1", "width": 1920, "height": 1080,
                "refresh_rate": 59.95, "x": -1536, "y": 100, "scale": 1.25,
                "disabled": false, "transform": 3, "workspaces": [1, 2]
            }]
        })
        .to_string(),
    )
    .unwrap();

    let applied = session.run(&["--preset", "desk"]);
    assert!(applied.status.success(), "{:?}", applied);
    let lua = session.saved_lua();
    assert!(lua.contains("position = \"-1536x100\", scale = 1.250000, transform = 3"));
    assert!(session
        .commands()
        .contains("workspace = \"2\", monitor = \"DP-1\""));
    let reloaded = session.run(&["--reload"]);
    assert!(reloaded.status.success(), "{:?}", reloaded);
    assert_eq!(session.saved_lua(), lua);
}

#[test]
fn lua_failure_is_reported_and_does_not_save_a_recent_layout() {
    let session = Session::new();
    fs::write(session.root.join("fail-eval"), "").unwrap();
    let result = session.run(&["--enable", "HDMI-A-1"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("invalid monitor spec"));
    assert!(!session.root.join("config/monitui/recent.json").exists());
}

#[test]
fn workspace_cli_uses_lua_and_propagates_evaluation_errors() {
    let session = Session::new();
    let result = session.run(&["--set-workspace", "5", "DP-1"]);
    assert!(result.status.success(), "{:?}", result);
    assert!(session.commands().contains(
        "eval\nhl.dispatch(hl.dsp.workspace.move({ workspace = \"5\", monitor = \"DP-1\" }))"
    ));
    fs::write(session.root.join("fail-eval"), "").unwrap();
    let result = session.run(&["--set-workspace", "5", "DP-1"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("invalid monitor spec"));
}
