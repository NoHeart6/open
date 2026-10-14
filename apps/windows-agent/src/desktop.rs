use anyhow::{Result, bail, ensure};
use serde_json::{Value, json};

#[cfg(any(windows, test))]
fn absolute_axis(pixel: i64, origin: i64, extent: i64) -> Result<i32> {
    ensure!(
        extent > 0 && pixel >= origin && pixel < origin + extent,
        "Pointer outside virtual screen"
    );
    Ok((((pixel - origin) * 65_536 + 32_768) / extent).min(65_535) as i32)
}

fn bounded_png(img: image::DynamicImage) -> Result<(Vec<u8>, u32, u32)> {
    use image::{
        ImageEncoder,
        codecs::png::{CompressionType, FilterType, PngEncoder},
    };
    let original = img.to_rgb8();
    let mut width = original.width().min(1280);
    let mut height = original.height().min(800);
    loop {
        let resized = image::imageops::thumbnail(&original, width, height);
        let mut output = Vec::new();
        PngEncoder::new_with_quality(&mut output, CompressionType::Best, FilterType::Adaptive)
            .write_image(
                resized.as_raw(),
                resized.width(),
                resized.height(),
                image::ExtendedColorType::Rgb8,
            )?;
        if output.len() <= 500_000 {
            return Ok((output, resized.width(), resized.height()));
        }
        width = (resized.width() * 3 / 4).max(1);
        height = (resized.height() * 3 / 4).max(1);
    }
}

fn bounded_jpeg(img: &image::DynamicImage) -> Result<(Vec<u8>, u32, u32)> {
    let original = img.to_rgb8();
    let mut width = original.width().min(960);
    let mut height = original.height().min(600);
    loop {
        let resized = image::imageops::thumbnail(&original, width, height);
        let mut output = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 70)
            .encode_image(&resized)?;
        if output.len() <= 100_000 {
            return Ok((output, resized.width(), resized.height()));
        }
        width = (resized.width() * 3 / 4).max(1);
        height = (resized.height() * 3 / 4).max(1);
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use std::{mem::size_of, ptr::null_mut};
    use windows_sys::Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{
            RemoteDesktop::*,
            StationsAndDesktops::*,
            Threading::{AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId},
        },
        UI::{HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    };

    pub fn available() -> Result<()> {
        unsafe {
            let mut session = 0;
            ensure!(
                ProcessIdToSessionId(GetCurrentProcessId(), &mut session) != 0 && session != 0,
                "Desktop automation requires a per-user session, not Session 0"
            );
            let mut buffer = null_mut();
            let mut size = 0;
            ensure!(
                WTSQuerySessionInformationW(
                    WTS_CURRENT_SERVER_HANDLE,
                    session,
                    WTSConnectState,
                    &mut buffer,
                    &mut size
                ) != 0,
                "Cannot inspect desktop session"
            );
            let active = size >= 4 && !buffer.is_null() && *(buffer as *const i32) == WTSActive;
            WTSFreeMemory(buffer.cast());
            ensure!(active, "Windows session is disconnected or unavailable");
            ensure!(
                !SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_null(),
                "Cannot establish physical desktop coordinate context"
            );
            let d = OpenInputDesktop(0, 0, DESKTOP_READOBJECTS);
            ensure!(!d.is_null(), "Interactive desktop unavailable or locked");
            let mut name = [0u16; 128];
            let mut needed = 0;
            let ok =
                GetUserObjectInformationW(d, UOI_NAME, name.as_mut_ptr().cast(), 256, &mut needed);
            CloseDesktop(d);
            ensure!(ok != 0, "Cannot identify input desktop");
            let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            ensure!(
                String::from_utf16_lossy(&name[..end]) == "Default",
                "Security or lock desktop: automation refused"
            );
        }
        Ok(())
    }
    pub fn consent(owner: &str, id: &str, action: &str, args: &Value) -> Result<bool> {
        available()?;
        let message = format!(
            "Account: {owner}\nTask: {id}\nAction: {action}\n\n{}\n\nAllow this exact operation once? Shell commands have all privileges of this Windows user.",
            serde_json::to_string(args)?
        );
        let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
        let title: Vec<u16> = format!("OpenCommander consent {id}")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let result = unsafe {
            MessageBoxW(
                null_mut(),
                text.as_ptr(),
                title.as_ptr(),
                MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2 | MB_SETFOREGROUND,
            )
        };
        Ok(result == IDYES)
    }
    pub fn close_consent(id: &str) -> Result<()> {
        let windows = windows()?;
        for w in windows["windows"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing windows"))?
        {
            if w["pid"].as_u64() == Some(u64::from(std::process::id()))
                && w["title"] == format!("OpenCommander consent {id}")
            {
                unsafe {
                    ensure!(
                        PostMessageW(
                            w["hwnd"].as_u64().unwrap_or(0) as HWND,
                            WM_COMMAND,
                            IDNO as usize,
                            0
                        ) != 0,
                        "Cannot dismiss expired consent"
                    );
                }
            }
        }
        Ok(())
    }
    unsafe extern "system" fn enumerate(hwnd: HWND, param: LPARAM) -> i32 {
        unsafe {
            if IsWindowVisible(hwnd) == 0 {
                return 1;
            }
            let mut title = [0u16; 512];
            let length = GetWindowTextW(hwnd, title.as_mut_ptr(), 512);
            if length == 0 {
                return 1;
            }
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect) == 0 {
                return 1;
            }
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            let list = &mut *(param as *mut Vec<Value>);
            if list.len() < 200 {
                list.push(json!({"hwnd":hwnd as usize,"pid":pid,"title":String::from_utf16_lossy(&title[..length as usize]),"active":GetForegroundWindow()==hwnd,"x":rect.left,"y":rect.top,"width":rect.right-rect.left,"height":rect.bottom-rect.top}));
            }
            1
        }
    }
    pub fn windows() -> Result<Value> {
        available()?;
        let mut list: Vec<Value> = vec![];
        unsafe {
            ensure!(
                EnumWindows(Some(enumerate), &mut list as *mut _ as LPARAM) != 0,
                "Window enumeration failed"
            );
        }
        Ok(json!({"windows":list,"virtual_screen":screen(),"coordinates":"physical pixels"}))
    }
    fn screen() -> Value {
        unsafe {
            json!({"x":GetSystemMetrics(SM_XVIRTUALSCREEN),"y":GetSystemMetrics(SM_YVIRTUALSCREEN),"width":GetSystemMetrics(SM_CXVIRTUALSCREEN),"height":GetSystemMetrics(SM_CYVIRTUALSCREEN),"monitor_count":GetSystemMetrics(SM_CMONITORS)})
        }
    }
    pub fn screenshot() -> Result<Value> {
        available()?;
        let dimensions = screen();
        let width = dimensions["width"].as_i64().unwrap() as i32;
        let height = dimensions["height"].as_i64().unwrap() as i32;
        ensure!(
            width > 0 && height > 0 && i64::from(width) * i64::from(height) <= 40_000_000,
            "Unsupported virtual screen size"
        );
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        unsafe {
            let screen_dc = GetDC(null_mut());
            ensure!(!screen_dc.is_null(), "Screen DC unavailable");
            let memory = CreateCompatibleDC(screen_dc);
            let bitmap = CreateCompatibleBitmap(screen_dc, width, height);
            if memory.is_null() || bitmap.is_null() {
                if !memory.is_null() {
                    DeleteDC(memory);
                }
                if !bitmap.is_null() {
                    DeleteObject(bitmap);
                }
                ReleaseDC(null_mut(), screen_dc);
                bail!("Capture allocation failed")
            }
            let old = SelectObject(memory, bitmap);
            let copied = BitBlt(
                memory,
                0,
                0,
                width,
                height,
                screen_dc,
                dimensions["x"].as_i64().unwrap() as i32,
                dimensions["y"].as_i64().unwrap() as i32,
                SRCCOPY | CAPTUREBLT,
            );
            SelectObject(memory, old);
            let mut info = BITMAPINFO::default();
            info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = width;
            info.bmiHeader.biHeight = -height;
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            info.bmiHeader.biCompression = BI_RGB;
            let lines = GetDIBits(
                memory,
                bitmap,
                0,
                height as u32,
                pixels.as_mut_ptr().cast(),
                &mut info,
                DIB_RGB_COLORS,
            );
            DeleteObject(bitmap);
            DeleteDC(memory);
            ReleaseDC(null_mut(), screen_dc);
            ensure!(copied != 0 && lines == height, "GDI capture failed");
        }
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }
        let img = image::RgbaImage::from_raw(width as u32, height as u32, pixels)
            .ok_or_else(|| anyhow::anyhow!("Invalid bitmap"))?;
        let img = image::DynamicImage::ImageRgba8(img);
        let (jpeg, jpeg_image_width, jpeg_image_height) = bounded_jpeg(&img)?;
        let (output, image_width, image_height) = bounded_png(img)?;
        Ok(
            json!({"png_base64":STANDARD.encode(output),"jpeg_base64":STANDARD.encode(jpeg),"jpeg_image_width":jpeg_image_width,"jpeg_image_height":jpeg_image_height,"virtual_screen":dimensions,"image_width":image_width,"image_height":image_height,"coordinates":"Use virtual_screen physical coordinates, not downscaled image pixels"}),
        )
    }
    fn send(inputs: &[INPUT]) -> Result<()> {
        unsafe {
            ensure!(
                SendInput(
                    inputs.len() as u32,
                    inputs.as_ptr(),
                    size_of::<INPUT>() as i32
                ) == inputs.len() as u32,
                "Input injection blocked by Windows permissions"
            );
        }
        Ok(())
    }
    fn mouse(flags: u32, data: u32) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: data,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }
    fn move_pointer(x: i64, y: i64) -> Result<()> {
        let v = screen();
        let mut input = mouse(
            MOUSEEVENTF_MOVE
                | MOUSEEVENTF_ABSOLUTE
                | MOUSEEVENTF_VIRTUALDESK
                | MOUSEEVENTF_MOVE_NOCOALESCE,
            0,
        );
        input.Anonymous.mi.dx =
            absolute_axis(x, v["x"].as_i64().unwrap(), v["width"].as_i64().unwrap())?;
        input.Anonymous.mi.dy =
            absolute_axis(y, v["y"].as_i64().unwrap(), v["height"].as_i64().unwrap())?;
        send(&[input])?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);
        loop {
            let mut point = POINT::default();
            if unsafe { GetPhysicalCursorPos(&mut point) } != 0
                && i64::from(point.x) == x
                && i64::from(point.y) == y
            {
                return Ok(());
            }
            ensure!(
                std::time::Instant::now() < deadline,
                "Pointer did not reach requested location"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    fn key(code: u16, scan: u16, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: code,
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }
    pub fn interact(a: &Value) -> Result<Value> {
        available()?;
        let swapped = unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0;
        let (primary_down, primary_up, secondary_down, secondary_up) = if swapped {
            (
                MOUSEEVENTF_RIGHTDOWN,
                MOUSEEVENTF_RIGHTUP,
                MOUSEEVENTF_LEFTDOWN,
                MOUSEEVENTF_LEFTUP,
            )
        } else {
            (
                MOUSEEVENTF_LEFTDOWN,
                MOUSEEVENTF_LEFTUP,
                MOUSEEVENTF_RIGHTDOWN,
                MOUSEEVENTF_RIGHTUP,
            )
        };
        let op = a["op"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing operation"))?;
        unsafe {
            let protected = if let Some(hwnd) = a["hwnd"].as_u64() {
                hwnd as HWND
            } else {
                GetForegroundWindow()
            };
            let mut pid = 0;
            GetWindowThreadProcessId(protected, &mut pid);
            ensure!(
                pid != GetCurrentProcessId(),
                "Remote input cannot operate OpenCommander permissions or consent"
            );
            if let (Some(x), Some(y)) = (a["x"].as_i64(), a["y"].as_i64()) {
                let hit = WindowFromPoint(POINT {
                    x: x as i32,
                    y: y as i32,
                });
                GetWindowThreadProcessId(hit, &mut pid);
                ensure!(
                    pid != GetCurrentProcessId(),
                    "Remote input cannot operate OpenCommander controls"
                );
            }
        }
        if op != "focus"
            && let Some(hwnd) = a["hwnd"].as_u64()
        {
            unsafe {
                ensure!(
                    GetForegroundWindow() == hwnd as HWND,
                    "Expected window is not foreground; input refused"
                );
            }
        }
        if matches!(
            op,
            "move" | "click" | "double_click" | "right_click" | "drag"
        ) && a.get("x").is_some()
        {
            let x = a["x"]
                .as_i64()
                .ok_or_else(|| anyhow::anyhow!("Missing x"))?;
            let y = a["y"]
                .as_i64()
                .ok_or_else(|| anyhow::anyhow!("Missing y"))?;
            let v = screen();
            let left = v["x"].as_i64().unwrap();
            let top = v["y"].as_i64().unwrap();
            ensure!(
                x >= left
                    && x < left + v["width"].as_i64().unwrap()
                    && y >= top
                    && y < top + v["height"].as_i64().unwrap(),
                "Pointer outside virtual screen"
            );
            move_pointer(x, y)?;
        }
        match op {
            "drag" => {
                let x = a["end_x"]
                    .as_i64()
                    .ok_or_else(|| anyhow::anyhow!("Missing drag end_x"))?;
                let y = a["end_y"]
                    .as_i64()
                    .ok_or_else(|| anyhow::anyhow!("Missing drag end_y"))?;
                let v = screen();
                let left = v["x"].as_i64().unwrap();
                let top = v["y"].as_i64().unwrap();
                ensure!(
                    x >= left
                        && x < left + v["width"].as_i64().unwrap()
                        && y >= top
                        && y < top + v["height"].as_i64().unwrap(),
                    "Drag endpoint outside virtual screen"
                );
                let mut start = POINT::default();
                unsafe {
                    GetPhysicalCursorPos(&mut start);
                    let hit = WindowFromPoint(POINT {
                        x: x as i32,
                        y: y as i32,
                    });
                    let mut pid = 0;
                    GetWindowThreadProcessId(hit, &mut pid);
                    ensure!(
                        pid != GetCurrentProcessId(),
                        "Cannot drag onto OpenCommander controls"
                    );
                }
                struct ReleaseButton(u32);
                impl Drop for ReleaseButton {
                    fn drop(&mut self) {
                        let _ = send(&[mouse(self.0, 0)]);
                    }
                }
                send(&[mouse(primary_down, 0)])?;
                let release = ReleaseButton(primary_up);
                for step in 1..=32 {
                    move_pointer(
                        i64::from(start.x) + (x - i64::from(start.x)) * step / 32,
                        i64::from(start.y) + (y - i64::from(start.y)) * step / 32,
                    )?;
                    std::thread::sleep(std::time::Duration::from_millis(15));
                }
                drop(release);
            }
            "focus" => {
                let hwnd = a["hwnd"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("Missing HWND"))?
                    as HWND;
                unsafe {
                    ensure!(IsWindow(hwnd) != 0, "Window no longer exists");
                    let current = GetCurrentThreadId();
                    let foreground = GetWindowThreadProcessId(GetForegroundWindow(), null_mut());
                    let mut message = MSG::default();
                    PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE);
                    let attached = foreground != 0
                        && foreground != current
                        && AttachThreadInput(current, foreground, 1) != 0;
                    ShowWindow(hwnd, SW_RESTORE);
                    BringWindowToTop(hwnd);
                    SetForegroundWindow(hwnd);
                    if attached {
                        AttachThreadInput(current, foreground, 0);
                    }
                    ensure!(
                        GetForegroundWindow() == hwnd,
                        "Windows foreground restrictions prevented focus"
                    );
                }
            }
            "move" => ensure!(
                a.get("x").is_some() && a.get("y").is_some(),
                "Move requires x and y"
            ),
            "click" => send(&[mouse(primary_down, 0), mouse(primary_up, 0)])?,
            "double_click" => send(&[
                mouse(primary_down, 0),
                mouse(primary_up, 0),
                mouse(primary_down, 0),
                mouse(primary_up, 0),
            ])?,
            "right_click" => send(&[mouse(secondary_down, 0), mouse(secondary_up, 0)])?,
            "scroll" => {
                let delta = a["delta"]
                    .as_i64()
                    .ok_or_else(|| anyhow::anyhow!("Missing scroll delta"))?;
                ensure!((-1200..=1200).contains(&delta), "Invalid scroll delta");
                send(&[mouse(MOUSEEVENTF_WHEEL, delta as u32)])?;
            }
            "type" => {
                let text = a["text"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing text"))?;
                ensure!(text.len() <= 4096, "Text size limit");
                let inputs: Vec<INPUT> = text
                    .encode_utf16()
                    .flat_map(|c| {
                        [
                            key(0, c, KEYEVENTF_UNICODE),
                            key(0, c, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
                        ]
                    })
                    .collect();
                send(&inputs)?;
            }
            "shortcut" => {
                let names = a["keys"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("Missing keys"))?;
                ensure!(!names.is_empty() && names.len() <= 5, "Invalid shortcut");
                let mut codes = vec![];
                for name in names {
                    let c = match name.as_str() {
                        Some("CTRL") => VK_CONTROL,
                        Some("ALT") => VK_MENU,
                        Some("SHIFT") => VK_SHIFT,
                        Some("WIN") => VK_LWIN,
                        Some("ENTER") => VK_RETURN,
                        Some("TAB") => VK_TAB,
                        Some("ESC") => VK_ESCAPE,
                        Some("A") => 0x41,
                        Some("C") => 0x43,
                        Some("V") => 0x56,
                        Some("S") => 0x53,
                        Some("F4") => VK_F4,
                        Some("BACKSPACE") => VK_BACK,
                        Some("DELETE") => VK_DELETE,
                        Some("INSERT") => VK_INSERT,
                        Some("HOME") => VK_HOME,
                        Some("END") => VK_END,
                        Some("PAGEUP") => VK_PRIOR,
                        Some("PAGEDOWN") => VK_NEXT,
                        Some("UP") => VK_UP,
                        Some("DOWN") => VK_DOWN,
                        Some("LEFT") => VK_LEFT,
                        Some("RIGHT") => VK_RIGHT,
                        Some(s)
                            if s.len() == 1
                                && s.bytes()
                                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()) =>
                        {
                            s.as_bytes()[0] as u16
                        }
                        Some(s) if s.starts_with('F') => {
                            let n = s[1..].parse::<u16>()?;
                            ensure!((1..=24).contains(&n), "Invalid function key");
                            VK_F1 + n - 1
                        }
                        _ => bail!("Unsupported shortcut key"),
                    };
                    codes.push(c)
                }
                let inputs: Vec<INPUT> = codes
                    .iter()
                    .map(|&c| key(c, 0, 0))
                    .chain(codes.iter().rev().map(|&c| key(c, 0, KEYEVENTF_KEYUP)))
                    .collect();
                send(&inputs)?;
            }
            _ => bail!("Unsupported desktop operation"),
        }
        let observed = windows()?;
        let mut pointer = POINT::default();
        unsafe {
            ensure!(
                GetPhysicalCursorPos(&mut pointer) != 0,
                "Cannot verify pointer position"
            );
        }
        if op == "move" {
            ensure!(
                a["x"].as_i64() == Some(i64::from(pointer.x))
                    && a["y"].as_i64() == Some(i64::from(pointer.y)),
                "Pointer did not reach requested location"
            );
        }
        Ok(
            json!({"operation":op,"input_sent":true,"pointer":{"x":pointer.x,"y":pointer.y},"foreground":observed["windows"].as_array().and_then(|w|w.iter().find(|w|w["active"]==true)),"verified":matches!(op,"focus"|"move"),"verification_state":if matches!(op,"focus"|"move"){"verified"}else{"sent"},"verification_note":"Input was sent. The attached screenshot and focused-control readback are observations; verify the requested application effect before reporting success."}),
        )
    }
}

#[cfg(windows)]
pub use native::*;
#[cfg(not(windows))]
pub fn consent(_: &str, _: &str, _: &str, _: &Value) -> Result<bool> {
    bail!("Desktop requires Windows")
}
#[cfg(not(windows))]
pub fn windows() -> Result<Value> {
    bail!("Desktop requires Windows")
}
#[cfg(not(windows))]
pub fn screenshot() -> Result<Value> {
    bail!("Desktop requires Windows")
}
#[cfg(not(windows))]
pub fn interact(_: &Value) -> Result<Value> {
    bail!("Desktop requires Windows")
}
#[cfg(not(windows))]
pub fn close_consent(_: &str) -> Result<()> {
    bail!("Desktop requires Windows")
}

#[cfg(test)]
mod screenshot_tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};

    #[test]
    fn physical_coordinates_map_to_virtual_desktop_pixel_centres() -> Result<()> {
        for (origin, extent) in [(0, 1920), (-1920, 3840), (-1200, 1200)] {
            for offset in 0..extent {
                let normalized = i64::from(absolute_axis(origin + offset, origin, extent)?);
                assert_eq!(normalized * extent / 65_536, offset);
            }
            assert!(absolute_axis(origin - 1, origin, extent).is_err());
            assert!(absolute_axis(origin + extent, origin, extent).is_err());
        }
        Ok(())
    }

    #[test]
    fn noisy_screenshot_is_valid_png_and_fits_wire_budget() -> Result<()> {
        let mut seed = 123456789u32;
        let image = image::RgbImage::from_fn(1280, 800, |_, _| {
            let mut rgb = [0; 3];
            for byte in &mut rgb {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                *byte = seed as u8;
            }
            image::Rgb(rgb)
        });
        let original = image::DynamicImage::ImageRgb8(image);
        let mut prior = std::io::Cursor::new(Vec::new());
        original.write_to(&mut prior, image::ImageFormat::Png)?;
        assert!(
            prior.get_ref().len() > 700_000,
            "Fixture must reproduce the old rejection"
        );
        let (png, width, height) = bounded_png(original)?;
        assert!(png.len() <= 700_000);
        assert!(width < 1280 && height < 800);
        let decoded = image::load_from_memory_with_format(&png, image::ImageFormat::Png)?;
        assert_eq!((decoded.width(), decoded.height()), (width, height));
        let wire =
            json!({"png_base64":STANDARD.encode(png),"image_width":width,"image_height":height});
        assert!(serde_json::to_vec(&wire)?.len() < 1_000_000);
        Ok(())
    }

    #[test]
    fn jpeg_fallback_decodes_and_fits_structured_client_budget() -> Result<()> {
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(1920, 1200, |x, y| {
            image::Rgb([(x % 251) as u8, (y % 253) as u8, ((x * y) % 255) as u8])
        }));
        let (jpeg, width, height) = bounded_jpeg(&image)?;
        assert!(jpeg.len() <= 100_000);
        assert!(STANDARD.encode(&jpeg).len() < 170_000);
        assert!(width <= 960 && height <= 600);
        let decoded = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg)?;
        assert_eq!((decoded.width(), decoded.height()), (width, height));
        Ok(())
    }

    #[test]
    fn small_screenshot_keeps_pixels_without_upscaling() -> Result<()> {
        let image = image::RgbImage::from_pixel(40, 20, image::Rgb([10, 20, 30]));
        let (png, width, height) = bounded_png(image::DynamicImage::ImageRgb8(image.clone()))?;
        assert_eq!((width, height), (40, 20));
        assert_eq!(image::load_from_memory(&png)?.to_rgb8(), image);
        Ok(())
    }
}
