use crate::ActionVmTest;
use anyhow::{anyhow, Result};
use camino::Utf8PathBuf;
use command_run::Command;
use fs_err as fs;
use rexpect::session::PtySession;
use std::env;

struct AlpineVersion {
    major: u32,
    minor: u32,
    patch: u32,
}

impl AlpineVersion {
    fn new(major: u32, minor: u32, patch: u32) -> AlpineVersion {
        AlpineVersion {
            major,
            minor,
            patch,
        }
    }

    fn iso_file_name(&self) -> String {
        format!(
            "alpine-virt-{}.{}.{}-x86_64.iso",
            self.major, self.minor, self.patch
        )
    }

    fn iso_url(&self) -> String {
        format!(
            "https://dl-cdn.alpinelinux.org/alpine/v{}.{}/releases/x86_64/{}",
            self.major,
            self.minor,
            self.iso_file_name()
        )
    }
}

struct Vm {
    qemu: String,
    iso_path: Utf8PathBuf,
    usb_backing_file: Utf8PathBuf,
    data_path: Utf8PathBuf,
}

fn session_run_cmd(p: &mut PtySession, cmd: &str) -> Result<()> {
    let prompt = "localhost:~# ";
    println!("{cmd}");
    p.exp_string(prompt)?;
    p.send_line(cmd)?;
    Ok(())
}

fn session_verify_success(p: &mut PtySession) -> Result<()> {
    session_run_cmd(p, "echo $?")?;
    // Skip the echo of the command.
    p.read_line()?;
    let exit_code = p.read_line()?;
    let exit_code = exit_code.trim();
    if exit_code != "0" {
        panic!("command exited non-zero: {}", exit_code);
    }
    Ok(())
}

impl Vm {
    fn run(&self, action: &ActionVmTest) -> Result<()> {
        let qemu_cmd = [
            self.qemu.as_str(),
            if action.disable_kvm {
                ""
            } else {
                "-enable-kvm"
            },
            "-display none",
            &format!("-drive format=raw,file={}", self.iso_path.as_str()),
            "-m 512",
            "-serial stdio",
            // Add a disk backed by the data subdirectory, will appear as sdb1.
            &format!("-drive format=raw,file=fat:rw:{}", self.data_path),
            // Add a USB disk device backed by a file.
            &format!(
                "-drive if=none,id=stick,format=raw,file={}",
                self.usb_backing_file
            ),
            "-device nec-usb-xhci,id=xhci",
            "-device usb-storage,bus=xhci.0,drive=stick",
        ]
        .join(" ");

        println!("{qemu_cmd}");

        // Use a fairly long timeout to avoid failing in the CI.
        let p = &mut rexpect::spawn(&qemu_cmd, Some(100_000))?;

        // Log in.
        println!("waiting to log in...");
        p.exp_string("localhost login: ")?;
        println!("logging in");
        p.send_line("root")?;

        // Mount the data dir.
        session_run_cmd(p, "mkdir /mnt/data")?;
        session_verify_success(p)?;
        session_run_cmd(p, "mount /dev/sdb1 /mnt/data")?;
        session_verify_success(p)?;

        // Run writedisk. Set the path to the data dir. This is necessary so
        // that the pseudo sudo executable will be found.
        session_run_cmd(p, "PATH=/mnt/data writedisk /mnt/data/test_file")?;

        // Skip the echo of the command.
        p.read_line()?;

        // Expect one USB device to be found.
        let choice_0 = p.read_line()?;
        if !choice_0.starts_with("0: [/dev/sdc] QEMU QEMU USB HARDDRIVE") {
            panic!("unexpected output: {}", choice_0);
        }

        // Choose the first disk and start the copy.
        println!("starting copy...");
        p.send_line("0")?;

        // Check the exit code.
        session_verify_success(p)?;

        // Shut down.
        session_run_cmd(p, "poweroff")?;
        p.exp_eof()?;

        Ok(())
    }
}

/// Get the absolute path of the repo. Assumes that this executable is
/// located at <repo>/target/<buildmode>/<exename>.
fn get_repo_path() -> Result<Utf8PathBuf> {
    let exe = Utf8PathBuf::from_path_buf(env::current_exe()?)
        .map_err(|_| anyhow!("exe path is not utf-8"))?;
    Ok(exe
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .ok_or_else(|| anyhow!("not enough parents: {}", exe))?
        .into())
}

pub fn run(action: &ActionVmTest) -> Result<()> {
    let repo_path = get_repo_path()?;

    let tmp_dir = tempfile::tempdir()?;
    let tmp_path = Utf8PathBuf::from_path_buf(tmp_dir.path().into()).unwrap();

    let alpine_version = AlpineVersion::new(3, 14, 2);

    // Download the Alpine ISO.
    let iso_path = tmp_path.join(alpine_version.iso_file_name());
    Command::with_args(
        "curl",
        ["--output", iso_path.as_str(), &alpine_version.iso_url()],
    )
    .run()?;

    // Create a file to back the virtual USB device.
    let usb_backing_file = tmp_path.join("usb_data");
    Command::with_args(
        "truncate",
        ["--size", "10MiB", usb_backing_file.as_str()],
    )
    .run()?;

    // Create the data dir.
    let data_path = tmp_path.join("data");
    fs::create_dir(&data_path)?;

    // Copy the pseudo sudo executable to the data dir.
    fs::copy(
        repo_path.join("xtask/src/pseudo_sudo"),
        data_path.join("sudo"),
    )?;

    // Build a statically-linked writedisk and copy to the data dir.
    Command::with_args(
        "cargo",
        [
            "build",
            "--release",
            "--target",
            "x86_64-unknown-linux-musl",
        ],
    )
    .run()?;
    let build_path = repo_path.join("target/x86_64-unknown-linux-musl/release");
    fs::copy(build_path.join("writedisk"), data_path.join("writedisk"))?;
    fs::copy(build_path.join("wd_copier"), data_path.join("wd_copier"))?;

    // Write out a test file that writedisk will use as input.
    let test_disk_data = b"This is the content of the test file";
    let test_data_path = data_path.join("test_file");
    fs::write(test_data_path, test_disk_data)?;

    // Run the VM.
    let vm = Vm {
        qemu: "qemu-system-x86_64".into(),
        iso_path,
        usb_backing_file: usb_backing_file.clone(),
        data_path,
    };
    vm.run(action).unwrap();

    // Verify the correct data was written. It should start with the
    // string in `test_disk_data` and the rest should be zeroes.
    let actual = fs::read(&usb_backing_file)?;
    assert_eq!(actual.len(), 10 * 1024 * 1024);
    assert_eq!(&actual[..test_disk_data.len()], test_disk_data);
    for byte in &actual[test_disk_data.len()..] {
        assert_eq!(*byte, 0);
    }

    println!("test passed");

    Ok(())
}
