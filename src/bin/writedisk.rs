#![warn(clippy::pedantic)]

use clap::Parser;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::{env, fs, process};

#[derive(Clone, Debug)]
struct UsbBlockDevice {
    /// The device path, e.g. "/dev/sdc"
    path: PathBuf,

    manufacturer: String,
    product: String,
    serial: String,
}

/// Try to determine whether this is a USB device or not by searching
/// upwards for a directory name starting with "usb".
fn is_usb_in_path(path: &Path) -> bool {
    for path in path.ancestors() {
        if let Some(name) = path.file_name()
            && let Some(name) = name.to_str()
            && name.starts_with("usb")
        {
            return true;
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
                    path: Path::new("/dev").join(entry.file_name()),
                    manufacturer: read("manufacturer")?,
                    product: read("product")?,
                    serial: read("serial")?,
                });
            }
        }
        Ok(result)
    }

    fn full_name(&self) -> String {
        format!("{} {} {}", &self.manufacturer, &self.product, &self.serial)
    }
}

fn choose_device(device_name: Option<&String>) -> UsbBlockDevice {
    let devices = UsbBlockDevice::get_all().unwrap();

    if devices.is_empty() {
        println!("no devices found");
        process::exit(1);
    }

    if let Some(device_name) = device_name {
        if let Some(device) = devices
            .iter()
            .find(|device| device.full_name() == *device_name)
        {
            println!(
                "writing to {} ({})",
                device.path.display(),
                device.full_name()
            );
            return device.clone();
        }

        println!("invalid device");
        process::exit(1);
    }

    for (index, device) in devices.iter().enumerate() {
        println!(
            "{index}: [{path}] {name}",
            path = device.path.display(),
            name = device.full_name()
        );
    }

    print!("select device: ");
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();

    let index = match input.trim().parse::<usize>() {
        Ok(i) => i,
        Err(_e) => {
            println!("invalid input");
            process::exit(1);
        }
    };

    if index >= devices.len() {
        println!("invalid index");
        process::exit(1);
    }

    devices[index].clone()
}

#[derive(Debug, Parser)]
#[command(about = "Write a disk image to a USB disk.", version)]
struct Opt {
    /// Disk image
    input: PathBuf,

    /// Full name of the target USB disk, e.g. "Samsung PSSD T7 S1SLVX2T1210".
    ///
    /// If not specified, available USB disks will be listed and an
    /// interactive choice must be made. (This interactive list includes
    /// the device name, so run the tool once without this argument to
    /// find the right device name.) Specifying the device name allows
    /// the tool to be used non-interactively.
    #[arg(long)]
    device_name: Option<String>,
}

fn main() {
    let opt = Opt::parse();

    // Check if the input file exists before doing anything else.
    if !opt.input.exists() {
        eprintln!("file not found: {}", opt.input.display());
        process::exit(1);
    }

    let device = choose_device(opt.device_name.as_ref());

    let copier_path = env::current_exe()
        .expect("failed to get current exe path")
        .parent()
        .expect("failed to get current exe directory")
        .join("wd_copier");

    println!(
        "sudo {} {} {}",
        copier_path.display(),
        opt.input.display(),
        device.path.display()
    );
    let status = process::Command::new("sudo")
        .args([&copier_path, &opt.input, &device.path])
        .status()
        .expect("failed to run command");
    if !status.success() {
        println!("copy failed");
        process::exit(1);
    }
}
