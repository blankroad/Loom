//! Bit placement of one signal in the payload.
//!
//! Positions follow the DBC convention: payload bit `p` is byte `p / 8`, bit `p % 8`
//! (bit 0 = least significant bit of the byte).
//! - Intel (`@1`): `start` is the LSB position; the value grows towards higher positions.
//! - Motorola (`@0`): `start` is the MSB position in that numbering ("sawtooth"); the value
//!   continues to lower bits of the same byte, then bit 7 of the next byte.
//!
//! A signal spans at most 9 bytes (64 bits + 7 bit shift), so one `u128` window is enough.

/// Byte order of a signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ByteOrder {
    /// Little endian, DBC `@1`.
    Intel,
    /// Big endian, DBC `@0`.
    Motorola,
}

/// Largest payload in bits (CAN FD 64 bytes).
pub(crate) const MAX_PAYLOAD_BITS: u64 = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BitLayout {
    order: ByteOrder,
    start: u16,
    size: u8,
    first_byte: u8,
    last_byte: u8,
    /// Right shift of the window to bring the LSB to bit 0.
    shift: u8,
}

impl BitLayout {
    /// `None` if size is 0 or above 64, or the signal leaves a 64-byte payload.
    pub(crate) fn new(order: ByteOrder, start: u64, size: u64) -> Option<Self> {
        if size == 0 || size > 64 || start >= MAX_PAYLOAD_BITS {
            return None;
        }
        let (first, last_byte, shift) = match order {
            ByteOrder::Intel => {
                let last = start + size - 1;
                if last >= MAX_PAYLOAD_BITS {
                    return None;
                }
                (start / 8, last / 8, start % 8)
            }
            ByteOrder::Motorola => {
                // Big-endian bit index: 0 = MSB of byte 0, counting towards the LSB.
                let msb = (start / 8) * 8 + 7 - start % 8;
                let last = msb + size - 1;
                if last >= MAX_PAYLOAD_BITS {
                    return None;
                }
                let last_byte = last / 8;
                (msb / 8, last_byte, (last_byte + 1) * 8 - 1 - last)
            }
        };
        Some(Self {
            order,
            start: start as u16,
            size: size as u8,
            first_byte: first as u8,
            last_byte: last_byte as u8,
            shift: shift as u8,
        })
    }

    pub(crate) fn order(&self) -> ByteOrder {
        self.order
    }

    pub(crate) fn start(&self) -> u16 {
        self.start
    }

    pub(crate) fn size(&self) -> u8 {
        self.size
    }

    /// Payload bytes needed to hold the whole signal.
    pub(crate) fn required_len(&self) -> usize {
        self.last_byte as usize + 1
    }

    pub(crate) fn mask(&self) -> u64 {
        if self.size == 64 {
            u64::MAX
        } else {
            (1u64 << self.size) - 1
        }
    }

    fn read_window(&self, data: &[u8]) -> u128 {
        let bytes = &data[self.first_byte as usize..=self.last_byte as usize];
        match self.order {
            ByteOrder::Intel => bytes.iter().rev().fold(0u128, |acc, &b| (acc << 8) | b as u128),
            ByteOrder::Motorola => bytes.iter().fold(0u128, |acc, &b| (acc << 8) | b as u128),
        }
    }

    fn write_window(&self, data: &mut [u8], window: u128) {
        let first = self.first_byte as usize;
        let last = self.last_byte as usize;
        for i in 0..=(last - first) {
            let byte = (window >> (8 * i)) as u8;
            match self.order {
                ByteOrder::Intel => data[first + i] = byte,
                ByteOrder::Motorola => data[last - i] = byte,
            }
        }
    }

    /// Raw bits (unsigned, not sign-extended). `None` if `data` is too short.
    pub(crate) fn extract(&self, data: &[u8]) -> Option<u64> {
        if data.len() < self.required_len() {
            return None;
        }
        let v = self.read_window(data) >> self.shift;
        Some((v as u64) & self.mask())
    }

    /// Writes `raw` (low `size` bits) into `data`, leaving other bits untouched.
    /// Caller guarantees `data.len() >= required_len()`.
    pub(crate) fn insert(&self, data: &mut [u8], raw: u64) {
        let mask = (self.mask() as u128) << self.shift;
        let mut v = self.read_window(data);
        v = (v & !mask) | (((raw & self.mask()) as u128) << self.shift);
        self.write_window(data, v);
    }

    /// Clears the signal's bits.
    pub(crate) fn clear(&self, data: &mut [u8]) {
        if data.len() >= self.required_len() {
            self.insert(data, 0);
        }
    }

    /// Occupied payload bit positions as a 512-bit set (for overlap checks).
    pub(crate) fn occupancy(&self) -> [u64; 8] {
        let mut set = [0u64; 8];
        let mut mark = |pos: usize| set[pos / 64] |= 1u64 << (pos % 64);
        let start = self.start as usize;
        match self.order {
            ByteOrder::Intel => (start..start + self.size as usize).for_each(&mut mark),
            ByteOrder::Motorola => {
                let msb = (start / 8) * 8 + 7 - start % 8;
                for idx in msb..msb + self.size as usize {
                    mark((idx / 8) * 8 + 7 - idx % 8);
                }
            }
        }
        set
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_layouts() {
        assert!(BitLayout::new(ByteOrder::Intel, 0, 0).is_none());
        assert!(BitLayout::new(ByteOrder::Intel, 0, 65).is_none());
        assert!(BitLayout::new(ByteOrder::Intel, 505, 8).is_none());
        assert!(BitLayout::new(ByteOrder::Intel, 504, 8).is_some());
        // Motorola MSB at byte 63 bit 0 can hold exactly 1 bit.
        assert!(BitLayout::new(ByteOrder::Motorola, 504, 1).is_some());
        assert!(BitLayout::new(ByteOrder::Motorola, 504, 2).is_none());
    }

    #[test]
    fn intel_across_bytes() {
        // 12 bits starting at bit 4: byte0 bits 7..4 are the low nibble, byte1 the high byte.
        let l = BitLayout::new(ByteOrder::Intel, 4, 12).unwrap();
        let mut d = [0u8; 2];
        l.insert(&mut d, 0xABC);
        assert_eq!(d, [0xC0, 0xAB]);
        assert_eq!(l.extract(&d), Some(0xABC));
        assert_eq!(l.required_len(), 2);
    }

    #[test]
    fn motorola_sawtooth() {
        // MSB at byte0 bit 3, 12 bits: byte0 bits 3..0, then byte1 bits 7..0.
        let l = BitLayout::new(ByteOrder::Motorola, 3, 12).unwrap();
        let mut d = [0u8; 2];
        l.insert(&mut d, 0xABC);
        assert_eq!(d, [0x0A, 0xBC]);
        assert_eq!(l.extract(&d), Some(0xABC));
    }

    #[test]
    fn insert_keeps_neighbours() {
        let l = BitLayout::new(ByteOrder::Intel, 8, 8).unwrap();
        let mut d = [0xFFu8; 3];
        l.insert(&mut d, 0x00);
        assert_eq!(d, [0xFF, 0x00, 0xFF]);
    }

    #[test]
    fn occupancy_matches_positions() {
        let l = BitLayout::new(ByteOrder::Motorola, 7, 9).unwrap();
        // byte0 all bits (0..=7) + byte1 bit 7 (position 15).
        let occ = l.occupancy();
        assert_eq!(occ[0], 0xFF | (1 << 15));
    }

    #[test]
    fn short_data() {
        let l = BitLayout::new(ByteOrder::Intel, 8, 8).unwrap();
        assert_eq!(l.extract(&[0x01]), None);
    }
}
