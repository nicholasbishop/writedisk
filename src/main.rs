// First we need to list all the drives. How do?
// something like: ls /sys/block/
//   exclude the ones that don't have a device subdirectory
//

use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process,
};
use structopt::StructOpt;

#[derive(Debug)]
struct UsbBlockDevice {
    /// The device path, e.g. "/dev/sdc"
    device: PathBuf,

    manufacturer: String,
    product: String,
    serial: String,
}

impl UsbBlockDevice {
    fn get_all() -> io::Result<Vec<UsbBlockDevice>> {
        let mut result = Vec::new();
        for entry in fs::read_dir("/sys/block")? {
            let entry = entry?;
            let path = entry.path();
            let device_path = path.join("device");
            if !device_path.exists() {
                continue;
            }
            // TODO(nicholasbishop): I have no idea if this will work
            // in the general case, I just got this by looking at one
            // machine.
            let device_path = device_path.canonicalize()?;
            let bus = device_path.join("../../../../..").canonicalize()?;
            if let Some(name) = bus.file_name() {
                if let Some(name) = name.to_str() {
                    if !name.starts_with("usb") {
                        continue;
                    }
                }
            }
            let usb_device = device_path.join("../../../..").canonicalize()?;
            let read = |name| -> io::Result<String> {
                Ok(fs::read_to_string(usb_device.join(name))?
                    .trim()
                    .to_string())
            };
            result.push(UsbBlockDevice {
                device: Path::new("/dev").join(entry.file_name()),
                manufacturer: read("manufacturer")?,
                product: read("product")?,
                serial: read("serial")?,
            });
        }
        Ok(result)
    }

    fn summary(&self) -> String {
        format!(
            "[{}] {} {} {}",
            self.device.display(),
            &self.manufacturer,
            &self.product,
            &self.serial,
        )
    }
}

#[derive(Debug, StructOpt)]
#[structopt(name = "writedisk", about = "Write a disk image to a USB disk.")]
struct Opt {
    /// Disk image
    input: PathBuf,
}

fn main() {
    let opt = Opt::from_args();

    let devices = UsbBlockDevice::get_all().unwrap();
    for (index, device) in devices.iter().enumerate() {
        println!("{}: {}", index, device.summary());
    }

    print!("select device: ");
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();

    let index = input.trim().parse::<usize>().unwrap();
    if index >= devices.len() {
        println!("invalid index");
        process::exit(0);
    }
    let device = &devices[index];

    let mut progress_bar = progress::Bar::new();

    let mut src = fs::File::open(opt.input).unwrap();
    let src_size = src.metadata().unwrap().len();

    let mut dst = fs::OpenOptions::new()
        .write(true)
        .open(&device.device)
        .unwrap();

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

    println!("syncing...");
    // Sync the data otherwise the speed/progress calculation will
    // be off
    dst.sync_data().unwrap();

    println!("finished");
}
