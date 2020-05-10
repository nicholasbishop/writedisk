# writedisk

Small utility for writing to a disk image to a USB drive.

Usage: `writedisk <input>`

This will scan for connected USB disks and prompt for you to select
one. Then the input file will be copied to the drive. The copying
operation is done with a small `wd_copier` binary that is
automatically invoked with `sudo`.

Linux only for now.

## Installation

    cargo install writedisk
