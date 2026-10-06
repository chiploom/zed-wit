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
    let mut input =
        Enigo::new(&settings).map_err(|error| format!("initialize input backend: {error}"))?;
    #[cfg(target_os = "linux")]
    input.set_delay(20);
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

    press(&mut input, Key::End, settle)?;
    press(&mut input, Key::F14, settle)?;
    press(&mut input, Key::F15, settle)?;
    type_text(&mut input, "gui", settle)?;
    press(&mut input, Key::F16, settle)?;
    type_text(&mut input, "snippet", settle)?;
    press(&mut input, Key::F16, settle)?;
    type_text(&mut input, "1.2.3", settle)?;
    press(&mut input, Key::F17, settle)?;
    type_text(&mut input, "reverse", settle)?;
    press(&mut input, Key::F16, settle)?;
    type_text(&mut input, "2.0.0", settle)?;
    press(&mut input, Key::F16, settle)?;
    type_text(&mut input, "\n// GUI_SNIPPET_FINAL", settle)?;
    press(&mut input, Key::F18, settle.saturating_mul(2))?;

    press(&mut input, Key::F19, settle)?;
    type_text(&mut input, "outline.wit", settle)?;
    press(&mut input, Key::Return, settle.saturating_mul(2))?;

    for (symbol, marker) in [
        ("alpha", "GUI_OUTLINE_ALPHA"),
        ("beta", "GUI_OUTLINE_BETA"),
        ("gamma", "GUI_OUTLINE_GAMMA"),
    ] {
        press(&mut input, Key::F13, settle)?;
        type_text(&mut input, symbol, settle)?;
        press(&mut input, Key::Return, settle)?;
        press(&mut input, Key::End, settle)?;
        type_text(&mut input, &format!(" // {marker}"), settle)?;
        press(&mut input, Key::F18, settle)?;
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
