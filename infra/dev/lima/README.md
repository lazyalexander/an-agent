# Optional Linux developer machine

`arm-linux.yaml` provisions an ordinary ARM Linux machine on an Apple silicon
Mac. It is development infrastructure only. Native Linux users need neither
Lima nor this configuration. Host runtime policy stays in `an-agent-host` and
uses native kernel enforcement, not a VM-provider interface.

See [native Linux acceptance](../../../doc/qualification.md) for
setup, source transfer, testing and evidence boundaries. Existing Lima instances
keep their own copied configuration; moving this file does not modify, restart
or delete them. Do not recreate an instance merely to adopt this repository
layout. SPACE/KVM acceptance remains separate.
