#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Damage(u8);

impl Damage {
    const SURFACE: u8 = 1 << 0;
    const CANVAS: u8 = 1 << 1;
    const UI: u8 = 1 << 2;

    pub(crate) fn initial() -> Self {
        Self(Self::SURFACE | Self::CANVAS | Self::UI)
    }

    pub(crate) fn surface(&mut self) {
        self.0 |= Self::SURFACE;
    }

    pub(crate) fn canvas(&mut self) {
        self.0 |= Self::CANVAS;
    }

    pub(crate) fn ui(&mut self) {
        self.0 |= Self::UI;
    }

    pub(crate) const fn pending(self) -> bool {
        self.0 != 0
    }

    pub(crate) fn clear_presented(&mut self) {
        self.0 = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presenting_coalesces_all_damage_sources() {
        let mut damage = Damage::default();
        damage.canvas();
        damage.ui();
        damage.surface();
        assert!(damage.pending());
        damage.clear_presented();
        assert!(!damage.pending());
    }
}
