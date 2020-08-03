# Creating a Service for Neolink

This will guide you through creating a service for neolink that
can be used on linux machines such as ubuntu and debian
buster/stetch or any other os that uses systemd.

The general steps are:

1. Create an unprivileged user to run neolink
2. Set up neolink somewhere the unprivileged user can run it
3. Creating the service file

## Creating the unprivileged user

For this I will use the username neolinker. Any unused name would be fine.

```bash
sudo adduser --system --no-create-home --shell /bin/false neolinker
```

Depending on your flavour of linux `adduser` may have also created
a group of the same name. If it did not, you can create a group with:

```bash
sudo addgroup --system neolinker
```

## Setting up neolink

For this we will put neolink in `/usr/local/bin` and the config in `/usr/local/etc` but any directory readable by the `neolinker` user would be fine.

We will also secure the config file so that only neolinker (and root) can read it. We want to do this because it contains passwords.

```bash
sudo cp neolink /usr/local/bin/neolink
sudo cp my_config.toml /usr/local/etc/neolink_config.toml
sudo chmod 755 /usr/local/bin/neolink
sudo chown neolinker:neolinker /usr/local/etc/neolink_config.toml
sudo chmod 600 /usr/local/etc/neolink_config.toml
```

## Creating the service

We will create a systemd service. This service will need to point to our files for using neolink and also instruct it to start with our unprivileged user.

Create a file here `/etc/systemd/system/neolink.service` with the following contents (you will need admin privileges to write to this location):

```
[Install]
WantedBy=multi-user.target

[Unit]
Description=Neolink service

[Service]
Type=simple
ExecStart=/usr/local/bin/neolink --config /usr/local/etc/neolink_config.toml
Restart=on-failure
User=neolinker
Group=neolinker

```

And that's it

## Controlling the Service

You can now control the service with the usual commands

To start it use:

```bash
systemctl start neolink
```

To stop it use:

```bash
systemctl stop neolink
```

To check it's running use:

```bash
systemctl status neolink
```

To make it run at startup from now on:

```bash
systemctl enable neolink
```

You can check it's log file with

```bash
journalctl -xeu neolink
```
