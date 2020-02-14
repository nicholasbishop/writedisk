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

/// Try to determine whether this is a USB device or not by searching
/// upwards for a directory name starting with "usb".
fn is_usb_in_path(path: &Path) -> bool {
    for path in path.ancestors() {
        if let Some(name) = path.file_name() {
            if let Some(name) = name.to_str() {
                if name.starts_with("usb") {
                    return true;
                }
            }
        }
    }
    false
}

/// Search upwards for a directory containing device info
/// (manufacturer, product, and serial).
fn find_usb_info(path: &Path) -> Option<PathBuf> {
    for path in path.ancestors() {
        if path.join("manufacturer").exists()
            && path.join("product").exists()
            && path.join("serial").exists()
        {
            return Some(path.into());
        }
    }
    None
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

            // This will give a very long path such as:
            // /sys/devices/pci0000:00/0000:00:01.2/0000:02:00.0/
            //     0000:03:08.0/0000:08:00.3/usb4/4-3/4-3.2/4-3.2:1.0/
            //     host7/target7:0:0/7:0:0:0
            let device_path = device_path.canonicalize()?;

            // Skip non-USB devices
            if !is_usb_in_path(&device_path) {
                continue;
            }

            if let Some(info_path) = find_usb_info(&device_path) {
                let read = |name| -> io::Result<String> {
                    let path = info_path.join(name);
                    let contents = fs::read_to_string(path)?;
                    Ok(contents.trim().into())
                };

                result.push(UsbBlockDevice {
                    device: Path::new("/dev").join(entry.file_name()),
                    manufacturer: read("manufacturer")?,
                    product: read("product")?,
                    serial: read("serial")?,
                });
            }
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
    dst.sync_data().unwrap();

    println!("finished");
}
