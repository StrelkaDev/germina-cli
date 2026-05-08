use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

const PROMPT: &str = "germina> ";

static OUTPUT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static PROMPT_VISIBLE: AtomicBool = AtomicBool::new(false);

fn output_lock() -> &'static Mutex<()> {
    OUTPUT_LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_output() -> io::Result<std::sync::MutexGuard<'static, ()>> {
    output_lock()
        .lock()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "Output lock poisoned"))
}

pub(crate) fn set_prompt_visible(visible: bool) {
    PROMPT_VISIBLE.store(visible, Ordering::SeqCst);
}

pub(crate) fn print_prompt() -> io::Result<()> {
    let _guard = lock_output()?;
    let mut out = io::stdout();
    write!(out, "{PROMPT}")?;
    out.flush()?;
    PROMPT_VISIBLE.store(true, Ordering::SeqCst);
    Ok(())
}

pub(crate) fn print_line(message: impl AsRef<str>) -> io::Result<()> {
    let _guard = lock_output()?;
    let prompt_visible = PROMPT_VISIBLE.load(Ordering::SeqCst);
    let mut out = io::stdout();

    if prompt_visible {
        write!(out, "\r")?;
    }

    writeln!(out, "{}", message.as_ref())?;

    if prompt_visible {
        write!(out, "{PROMPT}")?;
        out.flush()?;
    }

    Ok(())
}
