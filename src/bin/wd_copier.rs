#![warn(clippy::pedantic)]

use clap::Parser;
use nix::mount::umount;
use procfs::Current;
use std::convert::TryInto;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;
use std::{fs, process, thread};

#[derive(Debug, Parser)]
struct Opt {
    src: PathBuf,
    dst: PathBuf,
}

/// Get OS dirty byte count using [`procfs::Meminfo`].
fn get_dirty_bytes() -> u64 {
    match procfs::Meminfo::current() {
        Ok(o) => o.dirty,
        Err(_e) => 0,
    }
}

struct DirtyInfo {
    /// Dirty bytes before the copy. This is the "goal".
    before_copy: u64,
    /// Dirty bytes after the copy.
    after_copy: u64,
    /// Current number of dirty bytes.
    current: u64,
}

impl DirtyInfo {
    /// Estimate the percent completion (between 0 and 100) of the sync
    /// operation.
    ///
    /// The estimate is based on the idea that the number of dirty bytes
    /// will be close to the value it was before the copy operation once
    /// sync has completed. After the copy completes, the `current`
    /// value will be the same as `after_copy`, and it should decrease
    /// as the sync is underway until it reaches `before_copy`.
    fn calc_sync_percent(&self) -> i32 {
        let current = self.current.saturating_sub(self.before_copy);
        let max = self.after_copy.saturating_sub(self.before_copy);

        // Flip the value because a lower number of dirty pages is
        // closer to completion.
        100 - calc_percent(current, max)
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn calc_percent(current: u64, max: u64) -> i32 {
    // Prevent division by zero.
    if max == 0 {
        return 0;
    }

    let percent = (current as f64) / (max as f64) * 100_f64;
    let percent = percent as i32;
    if percent > 100 { 100 } else { percent }
}

/// Draws a progress bar for a disk sync.
///
/// Uses the dirty value before our copy as our 'goal' by reading from
/// `/proc/meminfo`. This isn't an exact science and is just a rough estimate
/// of our completion.
///
/// Meant to be run on a thread parallel to the actual sync process and exits
/// after receiving a signal from main that the sync is complete.
fn sync_progress_bar(
    rx: &mpsc::Receiver<()>,
    mut progress_bar: progress::Bar,
    mut dirty: DirtyInfo,
) {
    progress_bar.set_job_title("syncing... (2/2)");
    loop {
        dirty.current = get_dirty_bytes();
        progress_bar.reach_percent(dirty.calc_sync_percent());
        thread::sleep(Duration::from_millis(500));
        if matches!(
            rx.try_recv(),
            Ok(()) | Err(mpsc::TryRecvError::Disconnected)
        ) {
            return;
        }
    }
}

fn unmount_all_partitions(device: &Path) {
    // Unmount all partitions mounted for the selected device.
    let device_name =
        device.to_str().expect("non-utf8 device path: {device:?}");
    let mounts = match procfs::mounts() {
        Ok(mounts) => mounts,
        Err(e) => {
            eprintln!("failed to get mounts: {e}");
            return;
        }
    };
    let mounted_parts: Vec<_> = mounts
        .into_iter()
        .filter(|x| x.fs_spec.starts_with(device_name))
        .collect();
    if !mounted_parts.is_empty() {
        eprintln!(
            "chosen device {device_name} has currently mounted partitions!",
        );
        for part in &mounted_parts {
            let dev_name = part.fs_spec.as_str();
            let mount_point = part.fs_file.as_str();
            match umount(mount_point) {
                Ok(()) => println!("unmounted {dev_name}."),
                Err(e) => eprintln!("error unmounting {dev_name}: {e}"),
            }
        }
    }
}

fn main() {
    let opt = Opt::parse();

    unmount_all_partitions(&opt.dst);

    let mut dirty = DirtyInfo {
        before_copy: get_dirty_bytes(),
        after_copy: 0,
        current: 0,
    };

    let mut src = fs::File::open(opt.src).unwrap();
    let src_size = src.metadata().unwrap().len();

    let open_result = fs::OpenOptions::new().write(true).open(&opt.dst);
    let mut dst = match open_result {
        Ok(fh) => fh,
        Err(error) => {
            eprintln!(
                "An error occurred while opening {} for writing: {}",
                opt.dst.display(),
                error
            );
            process::exit(1);
        }
    };

    let mut progress_bar = progress::Bar::new();
    progress_bar.set_job_title("copying... (1/2)");

    let mut remaining = src_size;
    let mut bytes_written: u64 = 0;
    let chunk_size: u64 = 1024 * 1024; // TODO
    let mut buf = Vec::new();
    while remaining > 0 {
        let percent = calc_percent(bytes_written, src_size);
        progress_bar.reach_percent(percent);

        let read_size = if chunk_size > remaining {
            remaining
        } else {
            chunk_size
        };
        buf.resize(read_size.try_into().unwrap(), 0);

        src.read_exact(&mut buf).unwrap();
        dst.write_all(&buf).unwrap();

        remaining -= read_size;
        bytes_written += read_size;
    }

    let (tx, rx) = mpsc::channel();
    dirty.after_copy = get_dirty_bytes() - dirty.before_copy;

    // If we can't get dirty bytes info we can just print 'syncing...' to the screen
    if dirty.after_copy == 0 {
        println!("syncing... (2/2)");
    } else {
        thread::spawn(move || {
            sync_progress_bar(&rx, progress_bar, dirty);
        });
    }

    dst.sync_data().unwrap();
    tx.send(()).unwrap();

    println!("finished");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calc_percent() {
        assert_eq!(calc_percent(0, 20), 0);
        assert_eq!(calc_percent(1, 20), 5);
        assert_eq!(calc_percent(20, 20), 100);

        // Check clamping.
        assert_eq!(calc_percent(100, 20), 100);

        // Check for division by zero.
        assert_eq!(calc_percent(100, 0), 0);
    }

    #[test]
    fn test_dirty_calc_percent() {
        let mut dirty = DirtyInfo {
            before_copy: 100,
            after_copy: 120,
            current: 120,
        };
        assert_eq!(dirty.calc_sync_percent(), 0);

        dirty.current = 105;
        assert_eq!(dirty.calc_sync_percent(), 75);

        dirty.current = 100;
        assert_eq!(dirty.calc_sync_percent(), 100);

        // Check clamping.
        dirty.current = 0;
        assert_eq!(dirty.calc_sync_percent(), 100);
        dirty.current = 200;
        assert_eq!(dirty.calc_sync_percent(), 0);
    }
}
