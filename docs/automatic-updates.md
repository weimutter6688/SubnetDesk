# Manual updates

SubnetDesk does not check for, download, or install updates from GitHub or any
other update service. The desktop update controls and background scheduler have
been removed. Existing update preferences are ignored and removed on load.

Distribute new installers or portable packages through your local network, VPN,
or offline media. Close running sessions before manually installing an update.
Keep existing configuration files to retain credentials and network rules.

The current LAN protocol is v2. Update the controller and controlled device
together; v1 peers are rejected because their nonce construction is unsafe.
Reload browser tabs after updating the host. Configuration formats for saved
accounts and device fingerprints remain compatible. See the
[security review](security-network-review.md) for the protocol changes.

GitHub URLs used by build dependencies and CI are development resources;
they are not runtime update connections.
