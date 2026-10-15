use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
pub fn run(policy: &crate::policy::Policy, args: &Value) -> Result<Value> {
    let mode = args["op"].as_str().unwrap_or("observe");
    ensure!(mode == "observe" || mode == "act", "Unsupported operation");
    let hwnd = args["hwnd"].as_u64();
    let observe = || -> Result<Value> {
        match hwnd {
            Some(h) => crate::automation::execute(
                &json!({"hwnd":h,"max_depth":5,"max_elements":100}),
                false,
            ),
            None => crate::desktop::windows(),
        }
    };
    let before = observe()?;
    let mut out = json!({"before":before,"operation":mode});
    if mode == "act" {
        let hwnd = hwnd.context("HWND required")?;
        let action = args["action"].as_str().context("Action required")?;
        ensure!(
            ["focus", "invoke", "select", "set_value"].contains(&action),
            "Invalid action"
        );
        let mut target = json!({"hwnd":hwnd,"op":action});
        for k in ["element_path", "automation_id", "name", "text"] {
            if !args[k].is_null() {
                target[k] = args[k].clone()
            }
        }
        policy.authorize("desktop_automation", &target)?;
        out["action_result"] = crate::automation::execute(&target, true)?;
        std::thread::sleep(std::time::Duration::from_millis(150));
        out["after"] = observe()?;
        let expected = args["expect_text"].as_str().unwrap_or("");
        let verified =
            !expected.is_empty() && serde_json::to_string(&out["after"])?.contains(expected);
        out["verification_state"] = json!(if verified { "verified" } else { "unverified" });
    }
    if args["include_screenshot"] == true {
        ensure!(policy.allow_screenshots, "Screenshot permission required");
        let s = crate::desktop::screenshot()?;
        out["screenshot"] = s;
    }
    Ok(out)
}
