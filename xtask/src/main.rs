mod vmtest;

use anyhow::Result;
use argh::FromArgs;

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
pub struct ActionVmTest {
    /// don't enable KVM
    #[argh(switch)]
    disable_kvm: bool,
}

fn main() -> Result<()> {
    let opt: Opt = argh::from_env();

    match &opt.action {
        Action::VmTest(action) => vmtest::run(action),
    }
}
