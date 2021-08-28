mod vmtest;

use anyhow::Error;
use argh::FromArgs;
use fehler::throws;

#[derive(FromArgs)]
/// Tasks for writedisk.
struct Opt {
    #[argh(subcommand)]
    action: Action,
}

#[derive(FromArgs)]
#[argh(subcommand)]
enum Action {
    VmTest(ActionVmTest),
}

/// Test writedisk in a VM.
#[derive(FromArgs)]
#[argh(subcommand, name = "vmtest")]
struct ActionVmTest {}

#[throws]
fn main() {
    let opt: Opt = argh::from_env();

    match opt.action {
        Action::VmTest(_) => vmtest::run()?,
    }
}
