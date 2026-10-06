use std::{env, process::ExitCode, time::Duration};

#[cfg(any(
    feature = "native",
    feature = "linux-x11",
    feature = "linux-wayland",
    feature = "linux-libei"
))]
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("zed-gui-input: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let script = args
        .next()
        .ok_or("usage: zed-gui-input <probe|all> [settle-milliseconds]")?;

    validate_backend_selection()?;
    match script.as_str() {
        "probe" => {
            if args.next().is_some() {
                return Err("usage: zed-gui-input probe".into());
            }
            probe_backend()
        }
        "all" => {
            let settle = args
                .next()
                .ok_or("usage: zed-gui-input all <settle-milliseconds>")?
                .parse::<u64>()
                .map_err(|error| format!("invalid settle milliseconds: {error}"))?;
            if args.next().is_some() {
                return Err("usage: zed-gui-input all <settle-milliseconds>".into());
            }
            if !(100..=5000).contains(&settle) {
                return Err("settle milliseconds must be between 100 and 5000".into());
            }
            run_all(Duration::from_millis(settle))
        }
        other => Err(format!("unknown input script {other:?}")),
    }
}

fn validate_backend_selection() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let count = usize::from(cfg!(feature = "linux-x11"))
            + usize::from(cfg!(feature = "linux-wayland"))
            + usize::from(cfg!(feature = "linux-libei"));
        if count != 1 {
            return Err(format!(
                "Linux input helper requires exactly one backend feature; enabled count: {count}"
            ));
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    if !cfg!(feature = "native") {
        return Err("native input helper feature is required on macOS and Windows".into());
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    return Err("GUI input helper supports Zed desktop hosts: macOS, Linux, and Windows".into());

    Ok(())
}

#[cfg(any(
    feature = "native",
    feature = "linux-x11",
    feature = "linux-wayland",
    feature = "linux-libei"
))]
fn input_device() -> Result<Enigo, String> {
    let mut settings = Settings::default();
    settings.open_prompt_to_get_permissions = false;
    let input =
        Enigo::new(&settings).map_err(|error| format!("initialize input backend: {error}"))?;
    #[cfg(target_os = "linux")]
    {
        let mut input = input;
        input.set_delay(20);
        return Ok(input);
    }
    #[cfg(not(target_os = "linux"))]
    Ok(input)
}

#[cfg(any(
    feature = "native",
    feature = "linux-x11",
    feature = "linux-wayland",
    feature = "linux-libei"
))]
fn probe_backend() -> Result<(), String> {
    let _input = input_device()?;
    Ok(())
}

#[cfg(not(any(
    feature = "native",
    feature = "linux-x11",
    feature = "linux-wayland",
    feature = "linux-libei"
)))]
fn probe_backend() -> Result<(), String> {
    Err("GUI input helper was built without an input backend feature".into())
}

#[cfg(any(
    feature = "native",
    feature = "linux-x11",
    feature = "linux-wayland",
    feature = "linux-libei"
))]
fn run_all(settle: Duration) -> Result<(), String> {
    let mut input = input_device()?;

    type_keys(&mut input, "wit-package", settle)?;
    std::thread::sleep(settle);
    press(&mut input, Key::Return, settle)?;
    type_text(&mut input, "gui", settle)?;
    press(&mut input, Key::Tab, settle)?;
    type_text(&mut input, "snippet", settle)?;
    press(&mut input, Key::Tab, settle)?;
    type_text(&mut input, "1.2.3", settle)?;
    shortcut(&mut input, &[Key::Shift], Key::Tab, settle)?;
    type_text(&mut input, "reverse", settle)?;
    press(&mut input, Key::Tab, settle)?;
    type_text(&mut input, "2.0.0", settle)?;
    press(&mut input, Key::Tab, settle)?;
    type_text(&mut input, "\n// GUI_SNIPPET_FINAL", settle)?;
    save(&mut input, settle.saturating_mul(2))?;

    file_finder(&mut input, settle)?;
    type_text(&mut input, "outline.wit", settle)?;
    press(&mut input, Key::Return, settle.saturating_mul(2))?;

    for (symbol, marker) in [
        ("alpha", "GUI_OUTLINE_ALPHA"),
        ("beta", "GUI_OUTLINE_BETA"),
        ("gamma", "GUI_OUTLINE_GAMMA"),
    ] {
        outline(&mut input, settle)?;
        type_text(&mut input, symbol, settle)?;
        press(&mut input, Key::Return, settle)?;
        press(&mut input, Key::End, settle)?;
        type_text(&mut input, &format!(" // {marker}"), settle)?;
        save(&mut input, settle)?;
    }

    Ok(())
}

#[cfg(not(any(
    feature = "native",
    feature = "linux-x11",
    feature = "linux-wayland",
    feature = "linux-libei"
)))]
fn run_all(_settle: Duration) -> Result<(), String> {
    Err("GUI input helper was built without an input backend feature".into())
}

#[cfg(any(
    feature = "native",
    feature = "linux-x11",
    feature = "linux-wayland",
    feature = "linux-libei"
))]
fn shortcut(
    input: &mut Enigo,
    modifiers: &[Key],
    key: Key,
    settle: Duration,
) -> Result<(), String> {
    let mut pressed = Vec::with_capacity(modifiers.len());
    for modifier in modifiers {
        if let Err(error) = input.key(*modifier, Direction::Press) {
            for pressed_modifier in pressed.iter().rev() {
                let _ = input.key(*pressed_modifier, Direction::Release);
            }
            return Err(format!("press modifier {modifier:?}: {error}"));
        }
        pressed.push(*modifier);
    }

    let key_result = input
        .key(key, Direction::Click)
        .map_err(|error| format!("press shortcut key {key:?}: {error}"));

    let mut release_error = None;
    for modifier in pressed.iter().rev() {
        if let Err(error) = input.key(*modifier, Direction::Release) {
            release_error = Some(format!("release modifier {modifier:?}: {error}"));
        }
    }

    key_result?;
    if let Some(error) = release_error {
        return Err(error);
    }
    std::thread::sleep(settle);
    Ok(())
}

#[cfg(any(
    feature = "native",
    feature = "linux-x11",
    feature = "linux-wayland",
    feature = "linux-libei"
))]
fn platform_primary_modifier() -> Key {
    if cfg!(target_os = "macos") {
        Key::Meta
    } else {
        Key::Control
    }
}

#[cfg(any(
    feature = "native",
    feature = "linux-x11",
    feature = "linux-wayland",
    feature = "linux-libei"
))]
fn save(input: &mut Enigo, settle: Duration) -> Result<(), String> {
    shortcut(
        input,
        &[platform_primary_modifier()],
        Key::Unicode('s'),
        settle,
    )
}

#[cfg(any(
    feature = "native",
    feature = "linux-x11",
    feature = "linux-wayland",
    feature = "linux-libei"
))]
fn file_finder(input: &mut Enigo, settle: Duration) -> Result<(), String> {
    shortcut(
        input,
        &[platform_primary_modifier()],
        Key::Unicode('p'),
        settle,
    )
}

#[cfg(any(
    feature = "native",
    feature = "linux-x11",
    feature = "linux-wayland",
    feature = "linux-libei"
))]
fn outline(input: &mut Enigo, settle: Duration) -> Result<(), String> {
    shortcut(
        input,
        &[platform_primary_modifier(), Key::Shift],
        Key::Unicode('o'),
        settle,
    )
}

#[cfg(any(
    feature = "native",
    feature = "linux-x11",
    feature = "linux-wayland",
    feature = "linux-libei"
))]
fn type_keys(input: &mut Enigo, text: &str, settle: Duration) -> Result<(), String> {
    for ch in text.chars() {
        input
            .key(Key::Unicode(ch), Direction::Click)
            .map_err(|error| format!("type key {ch:?}: {error}"))?;
        std::thread::sleep(Duration::from_millis(25));
    }
    std::thread::sleep(settle);
    Ok(())
}

#[cfg(any(
    feature = "native",
    feature = "linux-x11",
    feature = "linux-wayland",
    feature = "linux-libei"
))]
fn press(input: &mut Enigo, key: Key, settle: Duration) -> Result<(), String> {
    input
        .key(key, Direction::Click)
        .map_err(|error| format!("press {key:?}: {error}"))?;
    std::thread::sleep(settle);
    Ok(())
}

#[cfg(any(
    feature = "native",
    feature = "linux-x11",
    feature = "linux-wayland",
    feature = "linux-libei"
))]
fn type_text(input: &mut Enigo, text: &str, settle: Duration) -> Result<(), String> {
    input
        .text(text)
        .map_err(|error| format!("type {text:?}: {error}"))?;
    std::thread::sleep(settle);
    Ok(())
}
