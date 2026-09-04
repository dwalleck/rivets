use std::env;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn open_lock(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

fn try_lock_label(path: &Path) -> io::Result<&'static str> {
    let file = open_lock(path)?;
    match file.try_lock() {
        Ok(()) => Ok("ACQUIRED"),
        Err(TryLockError::WouldBlock) => Ok("BUSY"),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

fn hold(path: &Path) -> io::Result<()> {
    let file = open_lock(path)?;
    file.try_lock()?;
    println!("LOCKED");
    io::stdout().flush()?;
    thread::sleep(Duration::from_secs(60));
    Ok(())
}

fn temp_root() -> io::Result<PathBuf> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let root = env::temp_dir().join(format!("rivets-j13o-{}-{nonce}", std::process::id()));
    fs::create_dir(&root)?;
    Ok(root)
}

fn run_parent() -> io::Result<()> {
    let root = temp_root()?;
    let same = root.join("workspace-a.lock");
    let different = root.join("workspace-b.lock");
    let executable = env::current_exe()?;
    let mut holder = Command::new(&executable)
        .arg("hold")
        .arg(&same)
        .stdout(Stdio::piped())
        .spawn()?;
    let stdout = holder
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("holder stdout unavailable"))?;
    let mut ready = String::new();
    BufReader::new(stdout).read_line(&mut ready)?;
    if ready.trim() != "LOCKED" {
        return Err(io::Error::other(format!("holder did not lock: {ready:?}")));
    }

    println!("same_workspace={}", try_lock_label(&same)?);
    println!("different_workspace={}", try_lock_label(&different)?);
    holder.kill()?;
    holder.wait()?;
    println!("after_holder_exit={}", try_lock_label(&same)?);

    let first = open_lock(&same)?;
    first.try_lock()?;
    println!("same_process_second_fd={}", try_lock_label(&same)?);
    drop(first);

    fs::remove_dir_all(root)?;
    Ok(())
}

fn main() -> io::Result<()> {
    let mut args = env::args_os();
    let _program = args.next();
    match args.next().as_deref() {
        Some(mode) if mode == "hold" => {
            let path = args
                .next()
                .ok_or_else(|| io::Error::other("missing lock path"))?;
            hold(Path::new(&path))
        }
        None => run_parent(),
        Some(mode) => Err(io::Error::other(format!("unknown mode: {mode:?}"))),
    }
}
