# writedisk

Small utility for writing a disk image to a USB drive.

Usage: `writedisk <input>`

This will scan for connected USB disks and prompt for you to select
one. Then the input file will be copied to the drive. The copying
operation is done with a small `wd_copier` binary that is
automatically invoked with `sudo`.

Linux only for now.

## Installation

### Cargo

```shell
cargo install writedisk
```

### Nix/NixOS

Per user:

```shell
nix-env --install writedisk
```

System-wide:

```shell
environment.systemPackages = with pkgs; [ writedisk ];
```

## License

Apache 2.0
