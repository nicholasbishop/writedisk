use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process,
};
use structopt::StructOpt;

#[derive(Clone, Debug)]
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

fn choose_device() -> UsbBlockDevice {
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
        process::exit(1);
    }

    devices[index].clone()
}

#[derive(Debug, StructOpt)]
#[structopt(name = "writedisk", about = "Write a disk image to a USB disk.")]
struct Opt {
    /// Disk image
    input: PathBuf,
}

fn main() {
    let opt = Opt::from_args();

    let device = choose_device();

    let copier_path = env::current_exe()
        .expect("failed to get current exe path")
        .parent()
        .expect("failed to get current exe directory")
        .join("writedisk_copier");

    println!(
        "sudo {} {} {}",
        copier_path.display(),
        opt.input.display(),
        device.device.display()
    );
    let status = process::Command::new("sudo")
        .args(&[&copier_path, &opt.input, &device.device])
        .status()
        .expect("failed to run command");
    if !status.success() {
        println!("copy failed");
        process::exit(1);
    }
}
