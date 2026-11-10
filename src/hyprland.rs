use std::process::{Command, Output};

/// Quote arbitrary text as a Lua string, including connector names and selectors.
pub fn lua_string(value: &str) -> String {
    let mut quoted = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            ch if ch.is_control() && (ch as u32) <= 127 => {
                // Three digits keep a following digit from extending the escape.
                quoted.push_str(&format!("\\{:03}", ch as u32));
            }
            ch => quoted.push(ch),
        }
    }
    quoted.push('"');
    quoted
}

fn check_response(output: Output) -> Result<(), String> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // IPC/Lua failures can be reported on stdout even with a zero exit status.
    if output.status.success() && stdout.trim() == "ok" {
        Ok(())
    } else {
        Err(format!(
            "hyprctl failed ({}): {} {}",
            output.status,
            stdout.trim(),
            stderr.trim()
        )
        .trim()
        .to_string())
    }
}

pub fn eval(lua: &str) -> Result<(), String> {
    let output = Command::new("hyprctl")
        .args(["eval", lua])
        .output()
        .map_err(|e| format!("Failed to run hyprctl eval: {}", e))?;
    check_response(output)
}

pub fn reload() -> Result<(), String> {
    let output = Command::new("hyprctl")
        .arg("reload")
        .output()
        .map_err(|e| format!("Failed to run hyprctl reload: {}", e))?;
    check_response(output)
}

pub fn move_workspace(workspace: u32, monitor: &str) -> Result<(), String> {
    eval(&format!(
        "hl.dispatch(hl.dsp.workspace.move({{ workspace = {}, monitor = {} }}))",
        lua_string(&workspace.to_string()),
        lua_string(monitor)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    #[test]
    fn quotes_lua_strings_without_allowing_code_injection() {
        assert_eq!(
            lua_string("desc:Écran \"TV\"\\\n\r\0\t9"),
            "\"desc:Écran \\\"TV\\\"\\\\\\010\\013\\000\\0099\""
        );
    }

    #[test]
    fn detects_lua_errors_reported_with_a_successful_exit_status() {
        let result = check_response(Output {
            status: ExitStatus::from_raw(0),
            stdout: b"Lua error: invalid monitor spec\n".to_vec(),
            stderr: vec![],
        });
        assert!(result.unwrap_err().contains("invalid monitor spec"));
    }

    #[test]
    fn accepts_ok_and_reports_process_errors() {
        assert!(check_response(Output {
            status: ExitStatus::from_raw(0),
            stdout: b"ok\n".to_vec(),
            stderr: vec![],
        })
        .is_ok());
        assert!(check_response(Output {
            status: ExitStatus::from_raw(256),
            stdout: vec![],
            stderr: b"No Hyprland instance".to_vec(),
        })
        .unwrap_err()
        .contains("No Hyprland instance"));
    }
}
