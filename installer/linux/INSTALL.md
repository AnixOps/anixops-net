# AnixOps Net for Linux

The desktop client requires GTK 4.14 and libadwaita 1.5. The core runs as a systemd service.
On Debian-based systems, use the DEB to let the package manager manage the service.

For a manual installation, extract the archive into a directory you control, verify its
checksum against the release, then run these commands from the extracted directory:

```sh
sudo install -m 0755 bin/anixops-netd /usr/bin/anixops-netd
sudo install -Dm0644 lib/systemd/system/anixops-netd.service /etc/systemd/system/anixops-netd.service
sudo systemctl daemon-reload
sudo systemctl enable --now anixops-netd.service
```

Start `bin/anixops-net` as your desktop user. For upgrades of the manual installation,
repeat the file installation and run `sudo systemctl restart anixops-netd.service`.

To remove a manual installation, stop and disable the service, remove the unit and
daemon installed above, reload systemd and delete the extracted directory.
Subscriptions and settings are kept under `/var/lib/anixops-net`.
