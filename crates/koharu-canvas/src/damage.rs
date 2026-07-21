/// Tracks which stages of the next frame must be rebuilt.
///
/// The dependency rules live here instead of being repeated throughout
/// `Canvas`: a new target requires a content render, and a content render must
/// be copied to the output before overlays can be presented.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RenderDamage(u8);

impl RenderDamage {
    const TARGET: u8 = 1 << 0;
    const CONTENT: u8 = 1 << 1;
    const OVERLAY: u8 = 1 << 2;

    pub const fn initial() -> Self {
        Self(Self::CONTENT | Self::OVERLAY)
    }

    pub fn target(&mut self) {
        self.0 |= Self::TARGET | Self::CONTENT | Self::OVERLAY;
    }

    pub fn content(&mut self) {
        self.0 |= Self::CONTENT | Self::OVERLAY;
    }

    pub fn overlay(&mut self) {
        self.0 |= Self::OVERLAY;
    }

    pub const fn target_pending(self) -> bool {
        self.0 & Self::TARGET != 0
    }

    pub const fn content_pending(self) -> bool {
        self.0 & Self::CONTENT != 0
    }

    pub const fn overlay_pending(self) -> bool {
        self.0 & Self::OVERLAY != 0
    }

    pub fn clear_target(&mut self) {
        self.0 &= !Self::TARGET;
    }

    pub fn clear_content(&mut self) {
        self.0 &= !Self::CONTENT;
    }

    pub fn clear_overlay(&mut self) {
        self.0 &= !Self::OVERLAY;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_damage_includes_every_downstream_stage() {
        let mut damage = RenderDamage::default();
        damage.target();

        assert!(damage.target_pending());
        assert!(damage.content_pending());
        assert!(damage.overlay_pending());
    }

    #[test]
    fn content_damage_always_recomposes_the_overlay() {
        let mut damage = RenderDamage::default();
        damage.content();

        assert!(!damage.target_pending());
        assert!(damage.content_pending());
        assert!(damage.overlay_pending());
    }
}
