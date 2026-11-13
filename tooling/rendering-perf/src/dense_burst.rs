//! One persistent dense color revision every half second during ordinary
//! ancestor motion. Absolute source tick determines state even after skips.
pub fn epoch(tick: u64, hz: u32) -> u64 {
    tick / u64::from(hz / 2)
}

pub fn color(epoch: u64) -> u32 {
    // Unique for the bounded capture; unlike toggling this cannot erase two
    // skipped bursts. Keep opaque colors comparable in raster cost.
    ((0x4488ccu32.wrapping_add(epoch as u32 * 0x070b0d) & 0xffffff) << 8) | 0xff
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coalesced_source_keeps_dense_state_and_ordinary_ticks_are_equal() {
        for hz in [60, 120] {
            assert_eq!(epoch(u64::from(hz / 2 - 1), hz), 0);
            assert_eq!(epoch(u64::from(hz / 2), hz), 1);
            assert_eq!(epoch(u64::from(hz * 2 + 1), hz), 4);
            let colors: std::collections::BTreeSet<_> = (0..20).map(color).collect();
            assert_eq!(colors.len(), 20);
            assert_eq!(color(0), 0x4488ccff);
        }
    }
}
