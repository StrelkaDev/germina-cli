pub fn parse_on_off(s: &str) -> anyhow::Result<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "1" => Ok(true),
        "off" | "false" | "0" => Ok(false),
        _ => Err(anyhow::anyhow!(
            "Expected one of: on/off, true/false, 1/0; got: {}",
            s
        )),
    }
}
