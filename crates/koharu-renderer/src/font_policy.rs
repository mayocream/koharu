//! Renderer-owned fallback policy for characters commonly absent from text fonts.

/// Ordered families used after the requested text families and the platform's
/// default sans-serif family.
///
/// The defaults intentionally include both monochrome symbol fonts and emoji
/// fonts. Missing families are ignored by the platform font collection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FontFallbackPolicy {
    symbol_families: Vec<String>,
}

impl FontFallbackPolicy {
    /// Creates a policy from an ordered list, dropping empty and duplicate names.
    #[must_use]
    pub fn new(families: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let mut symbol_families = Vec::new();
        for family in families {
            let family = family.into();
            if family.is_empty()
                || symbol_families
                    .iter()
                    .any(|known: &String| known.eq_ignore_ascii_case(&family))
            {
                continue;
            }
            symbol_families.push(family);
        }
        Self { symbol_families }
    }

    /// Disables the renderer's explicit symbol-family fallbacks.
    #[must_use]
    pub const fn disabled() -> Self {
        Self {
            symbol_families: Vec::new(),
        }
    }

    #[must_use]
    pub fn symbol_families(&self) -> &[String] {
        &self.symbol_families
    }
}

impl Default for FontFallbackPolicy {
    fn default() -> Self {
        Self::new([
            "Segoe UI Symbol",
            "Segoe UI Emoji",
            "Noto Sans Symbols",
            "Noto Sans Symbols2",
            "Noto Color Emoji",
            "Apple Color Emoji",
            "Apple Symbols",
            "Symbola",
            "Arial Unicode MS",
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_preserves_order_and_removes_invalid_duplicates() {
        let policy = FontFallbackPolicy::new([
            "Noto Sans Symbols",
            "",
            "noto sans symbols",
            "Segoe UI Symbol",
        ]);

        assert_eq!(
            policy.symbol_families(),
            ["Noto Sans Symbols", "Segoe UI Symbol"]
        );
    }

    #[test]
    fn default_restores_the_symbol_and_emoji_families() {
        assert_eq!(
            FontFallbackPolicy::default().symbol_families(),
            [
                "Segoe UI Symbol",
                "Segoe UI Emoji",
                "Noto Sans Symbols",
                "Noto Sans Symbols2",
                "Noto Color Emoji",
                "Apple Color Emoji",
                "Apple Symbols",
                "Symbola",
                "Arial Unicode MS",
            ]
        );
    }
}
