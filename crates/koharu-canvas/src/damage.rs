/// Tracks which stages of the next frame must be rebuilt.
///
/// The dependency rules live here instead of being repeated throughout
/// `Canvas`: a new target always requires a Vello content render.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RenderDamage(u8);

impl RenderDamage {
    const TARGET: u8 = 1 << 0;
    const CONTENT: u8 = 1 << 1;

    pub const fn initial() -> Self {
        Self(Self::CONTENT)
    }

    pub fn target(&mut self) {
        self.0 |= Self::TARGET | Self::CONTENT;
    }

    pub fn content(&mut self) {
        self.0 |= Self::CONTENT;
    }

    pub const fn target_pending(self) -> bool {
        self.0 & Self::TARGET != 0
    }

    pub const fn content_pending(self) -> bool {
        self.0 & Self::CONTENT != 0
    }

    pub fn clear_target(&mut self) {
        self.0 &= !Self::TARGET;
    }

    pub fn clear_content(&mut self) {
        self.0 &= !Self::CONTENT;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_damage_includes_content() {
        let mut damage = RenderDamage::default();
        damage.target();

        assert!(damage.target_pending());
        assert!(damage.content_pending());
    }
}
