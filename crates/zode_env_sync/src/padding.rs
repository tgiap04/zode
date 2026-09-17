use crate::EnvCryptoError;

/// Every sealed block is a whole number of these.
///
/// Without it the stored length of a blob is a close reading of the file: how
/// many variables it holds, and — watched over time — when three were added at
/// two in the morning. The server is not supposed to learn either.
pub const PAD_BLOCK: usize = 4096;

/// `seq` (u64) + `payload_len` (u32), both little-endian.
const HEADER_BYTES: usize = 12;

/// The largest `.env` this format carries.
///
/// A real one is under a kilobyte. This is headroom, and it is what keeps a
/// padded block inside the server's 1 MiB blob limit once base64 has inflated
/// it by a third.
pub const MAX_PAYLOAD_BYTES: usize = 256 * 1024;

/// Frames one payload for sealing.
///
/// The whole returned block is a multiple of [`PAD_BLOCK`], header included —
/// padding the payload alone would leak the header's 12 bytes of slack back
/// into the length.
pub fn pack(seq: u64, payload: &[u8]) -> Result<Vec<u8>, EnvCryptoError> {
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(EnvCryptoError::TooLarge {
            bytes: payload.len(),
            limit: MAX_PAYLOAD_BYTES,
        });
    }

    let used = HEADER_BYTES + payload.len();
    let total = used.div_ceil(PAD_BLOCK) * PAD_BLOCK;

    let mut block = Vec::with_capacity(total);
    block.extend_from_slice(&seq.to_le_bytes());
    block.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    block.extend_from_slice(payload);
    block.resize(total, 0);
    Ok(block)
}

/// Reads one back.
///
/// Strict about every field it does not need to guess at: a block that is not
/// a whole number of [`PAD_BLOCK`], a length that runs past the end, or
/// padding carrying anything but zeroes all mean this block was not produced
/// by [`pack`]. Refused rather than repaired — the repaired version would be a
/// different `.env` than the one that was sealed.
pub fn unpack(block: &[u8]) -> Result<(u64, Vec<u8>), EnvCryptoError> {
    if block.len() < PAD_BLOCK || !block.len().is_multiple_of(PAD_BLOCK) {
        return Err(EnvCryptoError::Malformed(format!(
            "a sealed block is a multiple of {PAD_BLOCK} bytes, this one is {}",
            block.len()
        )));
    }

    let seq = u64::from_le_bytes(
        block[..8]
            .try_into()
            .map_err(|_| EnvCryptoError::Malformed("sequence number".into()))?,
    );
    let length = u32::from_le_bytes(
        block[8..HEADER_BYTES]
            .try_into()
            .map_err(|_| EnvCryptoError::Malformed("payload length".into()))?,
    ) as usize;

    let end = HEADER_BYTES
        .checked_add(length)
        .ok_or_else(|| EnvCryptoError::Malformed("payload length overflows".into()))?;
    if end > block.len() {
        return Err(EnvCryptoError::Malformed(format!(
            "the payload claims {length} bytes but the block holds {}",
            block.len() - HEADER_BYTES
        )));
    }

    if block[end..].iter().any(|byte| *byte != 0) {
        return Err(EnvCryptoError::Malformed(
            "the padding carries data, so this block did not come from Zode".into(),
        ));
    }

    Ok((seq, block[HEADER_BYTES..end].to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let (seq, payload) = unpack(&pack(7, b"DATABASE_URL=postgres://x").unwrap()).unwrap();
        assert_eq!(seq, 7);
        assert_eq!(payload, b"DATABASE_URL=postgres://x");
    }

    #[test]
    fn the_block_is_always_a_whole_number_of_pages() {
        // The boundaries are set by the 12-byte header, not by PAD_BLOCK
        // alone: 4084 payload bytes exactly fill one page.
        for (payload_len, expected) in [
            (0, PAD_BLOCK),
            (1, PAD_BLOCK),
            (PAD_BLOCK - HEADER_BYTES - 1, PAD_BLOCK),
            (PAD_BLOCK - HEADER_BYTES, PAD_BLOCK),
            (PAD_BLOCK - HEADER_BYTES + 1, PAD_BLOCK * 2),
            (PAD_BLOCK, PAD_BLOCK * 2),
        ] {
            let block = pack(1, &vec![0x41; payload_len]).unwrap();
            assert_eq!(block.len(), expected, "payload of {payload_len} bytes");
        }
    }

    #[test]
    fn two_files_of_different_size_can_share_a_length() {
        // The entire point of the padding, stated as a test rather than a
        // comment: an eight-byte file and a four-kilobyte file look the same.
        let small = pack(1, b"A=1").unwrap();
        let large = pack(1, &vec![0x41; 4000]).unwrap();
        assert_eq!(small.len(), large.len());
    }

    #[test]
    fn an_oversized_payload_is_refused_before_it_is_sealed() {
        assert!(matches!(
            pack(1, &vec![0u8; MAX_PAYLOAD_BYTES + 1]),
            Err(EnvCryptoError::TooLarge { .. })
        ));
    }

    #[test]
    fn a_block_that_is_not_a_whole_page_is_refused() {
        assert!(unpack(&[0u8; 100]).is_err());
        assert!(unpack(&[0u8; PAD_BLOCK + 1]).is_err());
        assert!(unpack(&[]).is_err());
    }

    #[test]
    fn a_length_running_past_the_block_is_refused() {
        let mut block = pack(1, b"short").unwrap();
        block[8..12].copy_from_slice(&(PAD_BLOCK as u32).to_le_bytes());
        assert!(unpack(&block).is_err());
    }

    #[test]
    fn data_hidden_in_the_padding_is_refused() {
        let mut block = pack(1, b"A=1").unwrap();
        let last = block.len() - 1;
        block[last] = 0x01;
        assert!(matches!(unpack(&block), Err(EnvCryptoError::Malformed(_))));
    }

    #[test]
    fn the_sequence_number_survives_the_round_trip() {
        for seq in [0, 1, u64::MAX] {
            assert_eq!(unpack(&pack(seq, b"x").unwrap()).unwrap().0, seq);
        }
    }
}
