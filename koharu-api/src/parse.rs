pub fn parse_hex_color(hex: &str) -> anyhow::Result<[u8; 4]> {
    let hex = hex.trim_start_matches('#');
    match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16)?;
            let g = u8::from_str_radix(&hex[2..4], 16)?;
            let b = u8::from_str_radix(&hex[4..6], 16)?;
            Ok([r, g, b, 255])
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16)?;
            let g = u8::from_str_radix(&hex[2..4], 16)?;
            let b = u8::from_str_radix(&hex[4..6], 16)?;
            let a = u8::from_str_radix(&hex[6..8], 16)?;
            Ok([r, g, b, a])
        }
        _ => anyhow::bail!("Invalid hex color: {hex}"),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_hex_color;

    #[test]
    fn parse_color_with_alpha() {
        assert_eq!(
            parse_hex_color("#10203040").unwrap(),
            [0x10, 0x20, 0x30, 0x40]
        );
    }

    #[test]
    fn parse_color_without_alpha() {
        assert_eq!(parse_hex_color("ffffff").unwrap(), [255, 255, 255, 255]);
    }
}
