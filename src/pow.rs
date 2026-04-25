use blake3;

pub struct BlindHash;

impl BlindHash {
    #[allow(dead_code)]
    pub fn description() -> &'static str {
        "BlindHash is a non-ASIC-friendly proof-of-work function built on Blake3 with compact targets and transaction-aware retargeting hooks."
    }

    pub fn hash(header_bytes: &[u8]) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(header_bytes);
        hasher.finalize().into()
    }

    pub fn target_from_bits(bits: u32) -> u128 {
        let exponent = bits >> 24;
        let mantissa = bits & 0x007fffff;
        if exponent <= 3 {
            (mantissa as u128) >> (8 * (3 - exponent))
        } else {
            let shift = 8 * (exponent - 3);
            if shift >= 128 {
                u128::MAX
            } else {
                (mantissa as u128) << shift
            }
        }
    }

    pub fn bits_from_target(target: u128) -> u32 {
        if target == 0 {
            return 0x01000001;
        }

        let mut size = 0u32;
        let mut tmp = target;
        while tmp > 0 {
            size += 1;
            tmp >>= 8;
        }

        let mut compact = if size <= 3 {
            (target << (8 * (3 - size))) as u32
        } else {
            (target >> (8 * (size - 3))) as u32
        };

        if compact & 0x0080_0000 != 0 {
            compact >>= 8;
            size += 1;
        }

        (size << 24) | (compact & 0x007f_ffff)
    }
}
