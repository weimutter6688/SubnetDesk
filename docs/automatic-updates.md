# Manual updates

SubnetDesk does not check for, download, or install updates from GitHub or any
other update service. The desktop update controls and background scheduler have
been removed. Existing update preferences are ignored and removed on load.

Distribute new installers or portable packages through your local network, VPN,
or offline media. Close running sessions before manually installing an update.
Within this fork, keep configuration files to retain credentials and network rules.

The current LAN protocol is v3. Update the controller and controlled device
together; v1/v2 peers are rejected without downgrade. Reload browser tabs after
updating the host. This fork uses independent data/keyring/service identities;
the first upgrade from the original SubnetDesk is a clean configuration boundary.
Configure credentials again and verify the new device fingerprint. The new MSI
does not remove or upgrade the old product. See [fork identity](fork-identity.md).

GitHub URLs used by build dependencies and CI are development resources;
they are not runtime update connections.
