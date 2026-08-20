use anyhow::Result;

pub(crate) fn get() -> Result<String> {
    for path in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        let Ok(value) = std::fs::read_to_string(path) else {
            continue;
        };
        let value = value.trim();
        if !value.is_empty() && value != "uninitialized" {
            return Ok(value.to_owned());
        }
    }
    anyhow::bail!("no Linux machine identifier is available")
}
