use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    sync::mpsc::{self, TryRecvError},
    thread,
    time::Duration,
};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt()]
struct Opt {
    src: PathBuf,
    dst: PathBuf,
}

fn main() {
    let opt = Opt::from_args();

    let meminfo = procfs::Meminfo::new().unwrap();
    let starting_dirty = meminfo.dirty;

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
        let percent = (bytes_written as f32 / src_size as f32) * 100f32;
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

    progress_bar.set_job_title("syncing... (2/2)");

    let (tx, rx) = mpsc::channel();

    let meminfo = procfs::Meminfo::new().unwrap();
    let dirty_after_copy = meminfo.dirty - starting_dirty;

    thread::spawn(move || loop {
        let meminfo = procfs::Meminfo::new().unwrap();
        let percent = 100
            - ((meminfo.dirty.saturating_sub(starting_dirty))
                / (dirty_after_copy / 100));
        progress_bar.reach_percent(percent as i32);
        thread::sleep(Duration::from_millis(500));
        match rx.try_recv() {
            Ok(_) | Err(TryRecvError::Disconnected) => {
                break;
            }
            Err(TryRecvError::Empty) => {}
        }
    });

    dst.sync_data().unwrap();
    tx.send(()).unwrap();

    println!("finished");
}
