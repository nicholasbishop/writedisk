use std::convert::TryInto;
use std::io::{Read, Write};
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use std::{fs, thread};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt()]
struct Opt {
    src: PathBuf,
    dst: PathBuf,
}

/// Get OS dirty byte count using [`procfs::Meminfo`].
fn get_dirty_bytes() -> u64 {
    match procfs::Meminfo::new() {
        Ok(o) => o.dirty,
        Err(_e) => 0,
    }
}

/// Calculates the percentage of RangeInclusive.max as it approaches
/// RangeInclusive.min.
fn calc_percent(current: u64, range: RangeInclusive<u64>) -> i32 {
    // Subtract min from current but clamp to 0u64
    let numerator = current.saturating_sub(*range.start());
    let denominator = range.end() / 100;
    if denominator == 0 {
        return 0;
    }
    let percent = 100 - (numerator / denominator);
    percent.try_into().unwrap()
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
    dirty_before_copy: u64,
    dirty_after_copy: u64,
) {
    progress_bar.set_job_title("syncing... (2/2)");
    loop {
        let percent = calc_percent(
            get_dirty_bytes(),
            RangeInclusive::new(dirty_before_copy, dirty_after_copy),
        );
        progress_bar.reach_percent(percent);
        thread::sleep(Duration::from_millis(500));
        if matches!(
            rx.try_recv(),
            Ok(_) | Err(mpsc::TryRecvError::Disconnected)
        ) {
            return;
        }
    }
}

fn main() {
    let opt = Opt::from_args();

    let dirty_before_copy = get_dirty_bytes();

    let mut progress_bar = progress::Bar::new();
    progress_bar.set_job_title("copying... (1/2)");

    let mut src = fs::File::open(opt.src).unwrap();
    let src_size = src.metadata().unwrap().len();

    let mut dst = fs::OpenOptions::new().write(true).open(&opt.dst).unwrap();

    let mut remaining = src_size;
    let mut bytes_written = 0;
    let chunk_size: u64 = 1024 * 1024; // TODO
    let mut buf = Vec::new();
    while remaining > 0 {
        let percent = (bytes_written as f32 / src_size as f32) * 100_f32;
        progress_bar.reach_percent(percent as i32);

        let read_size = if chunk_size > remaining {
            remaining
        } else {
            chunk_size
        };
        buf.resize(read_size as usize, 0);

        src.read_exact(&mut buf).unwrap();
        dst.write_all(&buf).unwrap();

        remaining -= read_size;
        bytes_written += read_size;
    }

    let (tx, rx) = mpsc::channel();
    let dirty_after_copy = get_dirty_bytes() - dirty_before_copy;

    // If we can't get dirty bytes info we can just print 'syncing...' to the screen
    if dirty_after_copy == 0 {
        println!("syncing... (2/2)");
    } else {
        thread::spawn(move || {
            sync_progress_bar(
                &rx,
                progress_bar,
                dirty_before_copy,
                dirty_after_copy,
            );
        });
    }

    dst.sync_data().unwrap();
    tx.send(()).unwrap();

    println!("finished");
}
