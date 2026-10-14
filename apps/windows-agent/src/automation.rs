use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

fn selector_path(a: &Value) -> Result<Option<Vec<usize>>> {
    let Some(path) = a.get("element_path") else {
        return Ok(None);
    };
    let path = path
        .as_array()
        .context("element_path must be an array from desktop_inspect")?;
    ensure!(path.len() <= 32, "UI element path exceeds depth budget");
    path.iter()
        .map(|index| {
            let index = index
                .as_u64()
                .context("UI element path indices must be nonnegative integers")?;
            ensure!(
                index < 200,
                "UI element path index exceeds traversal budget"
            );
            Ok(index as usize)
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
}
#[cfg(windows)]
pub fn execute(a: &Value, write: bool) -> Result<Value> {
    use windows::{
        Win32::{Foundation::HWND, System::Com::*, UI::Accessibility::*},
        core::{BSTR, Interface},
    };
    crate::desktop::available()?;
    let hwnd = a["hwnd"].as_u64().context("Missing window handle")?;
    let mut pid = 0;
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd as _, &mut pid);
    }
    ensure!(
        pid != 0 && pid != std::process::id(),
        "OpenCommander controls and invalid windows cannot be automated"
    );
    let id = a["automation_id"].as_str().filter(|s| !s.is_empty());
    let name = a["name"].as_str().filter(|s| !s.is_empty());
    let path = selector_path(a)?;
    let op = if write {
        a["op"].as_str().context("Missing automation operation")?
    } else {
        "inspect"
    };
    if write && op == "focus" && id.is_none() && name.is_none() && path.is_none() {
        crate::desktop::interact(&json!({"op":"focus","hwnd":hwnd}))?;
        return Ok(
            json!({"operation":"focus","target":"window","hwnd":hwnd,"verified":true,"verification_note":"Window foreground HWND was read back"}),
        );
    }
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let _apartment = Apartment;
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
        if let Ok(v2) = automation.cast::<IUIAutomation2>() {
            v2.SetConnectionTimeout(1500)?;
            v2.SetTransactionTimeout(1500)?;
        }
        let root = automation.ElementFromHandle(HWND(hwnd as _))?;
        let walker = automation.ControlViewWalker()?;
        let max = a["max_elements"].as_u64().unwrap_or(200).min(200) as usize;
        if !write {
            let mut elements = vec![];
            let mut truncated = false;
            collect(
                &walker,
                &root,
                &[],
                a["max_depth"].as_u64().unwrap_or(6).min(8) as usize,
                max,
                &mut elements,
                &mut truncated,
            )?;
            return Ok(
                json!({"hwnd":hwnd,"elements":elements.iter().map(|(e,p)|describe(e,Some(p))).collect::<Vec<_>>(),"limit":max,"truncated":truncated,"selector_note":"Copy element_path from this inspection. Reinspect after UI changes; include nonempty name/automation_id as identity checks.","password_values":"Excluded"}),
            );
        }
        ensure!(
            id.is_some() || name.is_some() || path.is_some(),
            "An exact automation_id, name or inspected element_path is required for control operations"
        );
        let element = if let Some(path) = &path {
            let mut element = root.clone();
            for &index in path {
                let mut child = walker
                    .GetFirstChildElement(&element)
                    .context("UI element path is stale; inspect the window again")?;
                for _ in 0..index {
                    child = walker
                        .GetNextSiblingElement(&child)
                        .context("UI element path is stale; inspect the window again")?;
                }
                element = child;
            }
            ensure!(
                id.is_none_or(|id| element.CurrentAutomationId().is_ok_and(|v| v == id))
                    && name.is_none_or(|name| element.CurrentName().is_ok_and(|v| v == name)),
                "UI element path identity changed; inspect the window again"
            );
            element
        } else {
            use windows::Win32::System::Variant::VARIANT;
            let mut conditions = vec![Some(automation.ControlViewCondition()?)];
            if let Some(id) = id {
                conditions.push(Some(automation.CreatePropertyCondition(
                    UIA_AutomationIdPropertyId,
                    &VARIANT::from(id),
                )?));
            }
            if let Some(name) = name {
                conditions.push(Some(
                    automation.CreatePropertyCondition(UIA_NamePropertyId, &VARIANT::from(name))?,
                ));
            }
            let condition = automation.CreateAndConditionFromNativeArray(&conditions)?;
            let matches = root.FindAll(TreeScope_Subtree, &condition)?;
            let count = matches.Length()?;
            ensure!(
                count > 0,
                "UI selector matched no controls; inspect this HWND again and use the returned element_path"
            );
            ensure!(
                count == 1,
                "UI selector is ambiguous ({count} controls); use element_path from desktop_inspect"
            );
            matches.GetElement(0)?
        };
        ensure!(
            element.CurrentProcessId()? as u32 != std::process::id(),
            "OpenCommander controls cannot be automated"
        );
        ensure!(
            !element.CurrentIsPassword()?.as_bool(),
            "Password elements require direct human interaction"
        );
        ensure!(
            element.CurrentIsEnabled()?.as_bool(),
            "UI element is disabled"
        );
        let before = describe(&element, path.as_deref());
        let verified = match op {
            "focus" => {
                crate::desktop::interact(&json!({"op":"focus","hwnd":hwnd}))?;
                if automation.CompareElements(&root, &element)?.as_bool() {
                    return Ok(
                        json!({"operation":op,"target":"window","hwnd":hwnd,"before":before,"verified":true,"verification_note":"Window foreground HWND was read back"}),
                    );
                }
                ensure!(
                    element.CurrentIsKeyboardFocusable()?.as_bool(),
                    "Selected control is not keyboard focusable; use window-only focus or inspect for a focusable control"
                );
                element.SetFocus()?;
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(750);
                loop {
                    if element.CurrentHasKeyboardFocus()?.as_bool() {
                        break true;
                    }
                    if std::time::Instant::now() >= deadline {
                        break false;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
            }
            "set_value" => {
                let text = a["text"].as_str().context("Missing value")?;
                ensure!(text.len() <= 512, "UI value exceeds consent budget");
                let value: IUIAutomationValuePattern =
                    element.GetCurrentPatternAs(UIA_ValuePatternId)?;
                value.SetValue(&BSTR::from(text))?;
                value.CurrentValue()? == text
            }
            "invoke" => {
                let invoke: IUIAutomationInvokePattern =
                    element.GetCurrentPatternAs(UIA_InvokePatternId)?;
                invoke.Invoke()?;
                false
            }
            "select" => {
                let selection: IUIAutomationSelectionItemPattern =
                    element.GetCurrentPatternAs(UIA_SelectionItemPatternId)?;
                selection.Select()?;
                selection.CurrentIsSelected()?.as_bool()
            }
            _ => anyhow::bail!("Unsupported UI Automation operation"),
        };
        if op != "invoke" {
            ensure!(verified, "UI Automation postcondition did not match");
        }
        let after = describe(&element, path.as_deref());
        Ok(
            json!({"operation":op,"before":before,"after":after,"verified":verified,"verification_note":if op=="invoke"{"Invocation was sent; inspect application state to verify the requested effect"}else{"Requested property was read back"}}),
        )
    }
}
#[cfg(windows)]
unsafe fn collect(
    walker: &windows::Win32::UI::Accessibility::IUIAutomationTreeWalker,
    element: &windows::Win32::UI::Accessibility::IUIAutomationElement,
    path: &[usize],
    max_depth: usize,
    max: usize,
    result: &mut Vec<(
        windows::Win32::UI::Accessibility::IUIAutomationElement,
        Vec<usize>,
    )>,
    truncated: &mut bool,
) -> Result<()> {
    unsafe {
        if result.len() >= max {
            *truncated = true;
            return Ok(());
        }
        result.push((element.clone(), path.to_vec()));
        if path.len() >= max_depth {
            *truncated |= walker.GetFirstChildElement(element).is_ok();
            return Ok(());
        }
        let mut child = walker.GetFirstChildElement(element).ok();
        let mut index = 0;
        while let Some(current) = child {
            let mut child_path = path.to_vec();
            child_path.push(index);
            collect(
                walker,
                &current,
                &child_path,
                max_depth,
                max,
                result,
                truncated,
            )?;
            if result.len() >= max {
                *truncated |= walker.GetNextSiblingElement(&current).is_ok();
                break;
            }
            child = walker.GetNextSiblingElement(&current).ok();
            index += 1;
        }
        Ok(())
    }
}
#[cfg(windows)]
unsafe fn describe(
    element: &windows::Win32::UI::Accessibility::IUIAutomationElement,
    path: Option<&[usize]>,
) -> Value {
    use windows::Win32::UI::Accessibility::*;
    unsafe {
        let password = element
            .CurrentIsPassword()
            .map(|v| v.as_bool())
            .unwrap_or(true);
        let value = if password {
            None
        } else {
            element
                .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                .ok()
                .and_then(|p| p.CurrentValue().ok())
                .map(|s| s.to_string().chars().take(4096).collect::<String>())
        };
        let rectangle = element
            .CurrentBoundingRectangle()
            .ok()
            .map(|r| json!({"x":r.left,"y":r.top,"width":r.right-r.left,"height":r.bottom-r.top}));
        json!({"depth":path.map(|p|p.len()),"element_path":path,"name":element.CurrentName().ok().map(|s|s.to_string()),"automation_id":element.CurrentAutomationId().ok().map(|s|s.to_string()),"control_type":element.CurrentControlType().ok().map(|s|s.0),"focused":element.CurrentHasKeyboardFocus().ok().map(|s|s.as_bool()),"focusable":element.CurrentIsKeyboardFocusable().ok().map(|s|s.as_bool()),"enabled":element.CurrentIsEnabled().ok().map(|s|s.as_bool()),"password":password,"value":value,"bounds":rectangle})
    }
}
#[cfg(not(windows))]
pub fn execute(_: &Value, _: bool) -> Result<Value> {
    anyhow::bail!("UI Automation requires Windows")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inspected_paths_accept_root_and_children_but_reject_invalid_indices() -> Result<()> {
        assert_eq!(selector_path(&json!({}))?, None);
        assert_eq!(selector_path(&json!({"element_path":[]}))?, Some(vec![]));
        assert_eq!(
            selector_path(&json!({"element_path":[1,0,2]}))?,
            Some(vec![1, 0, 2])
        );
        for value in [json!(-1), json!(200), json!(1.5), json!("0")] {
            assert!(selector_path(&json!({"element_path":[value]})).is_err());
        }
        assert!(selector_path(&json!({"element_path":vec![0;33]})).is_err());
        assert!(selector_path(&json!({"element_path":"0"})).is_err());
        Ok(())
    }
}
