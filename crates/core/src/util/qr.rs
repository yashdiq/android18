//! Minimal QR encoder for desktop-side pairing codes.
//!
//! Byte mode, ECC level M, versions 1–6 — up to 106 payload bytes, well
//! past the ~64-byte pairing JSON. Implemented with `std` only (the core
//! crate stays dependency-light): bitstream → block split + Reed–Solomon
//! ECC → interleave → function patterns → zigzag placement → best of the
//! eight masks by penalty score → format information. Versions 7+ are
//! deliberately unsupported (they need version-information blocks and the
//! pairing payload never needs that capacity).

use std::fmt;

/// Byte-mode mode indicator (`0100`).
const MODE_BITS: u32 = 4;
/// Char-count indicator width in bits for versions 1–9 (byte mode).
const COUNT_BITS: u32 = 8;

/// Encoding failed because the payload exceeds the supported capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QrTooLarge {
    /// Payload size that was requested, in bytes.
    pub needed: usize,
    /// Largest capacity this encoder supports (version 6-M), in bytes.
    pub max: usize,
}

impl fmt::Display for QrTooLarge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "payload is {} bytes; QR pairing codes cap at {} bytes",
            self.needed, self.max
        )
    }
}

impl std::error::Error for QrTooLarge {}

/// Per-version geometry at ECC level M.
struct VersionSpec {
    version: u8,
    /// Data-codeword count of each Reed–Solomon block, in order.
    blocks: &'static [u16],
    /// ECC codewords per block.
    ec_per_block: u16,
    /// Alignment-pattern center coordinates; empty for version 1.
    alignment: &'static [u16],
}
const VERSIONS: [VersionSpec; 6] = [
    VersionSpec {
        version: 1,
        blocks: &[16],
        ec_per_block: 10,
        alignment: &[],
    },
    VersionSpec {
        version: 2,
        blocks: &[28],
        ec_per_block: 16,
        alignment: &[6, 18],
    },
    VersionSpec {
        version: 3,
        blocks: &[44],
        ec_per_block: 26,
        alignment: &[6, 22],
    },
    VersionSpec {
        version: 4,
        blocks: &[32, 32],
        ec_per_block: 18,
        alignment: &[6, 26],
    },
    VersionSpec {
        version: 5,
        blocks: &[43, 43],
        ec_per_block: 24,
        alignment: &[6, 30],
    },
    VersionSpec {
        version: 6,
        blocks: &[27, 27, 27, 27],
        ec_per_block: 16,
        alignment: &[6, 34],
    },
];

impl VersionSpec {
    /// Total data codewords across blocks.
    fn data_codewords(&self) -> usize {
        self.blocks.iter().map(|&b| b as usize).sum()
    }

    /// Byte-mode payload capacity (mode + count indicator overhead).
    fn byte_capacity(&self) -> usize {
        (self.data_codewords() * 8 - MODE_BITS as usize - COUNT_BITS as usize) / 8
    }
}

/// GF(256) tables over the QR field polynomial `x^8+x^4+x^3+x^2+1`
/// (0x11D). `exp` is double-length so multiplication never wraps.
struct Galois {
    exp: [u8; 510],
    log: [u8; 256],
}

const GF: Galois = build_galois();

const fn build_galois() -> Galois {
    let mut exp = [0u8; 510];
    let mut log = [0u8; 256];
    let mut x: usize = 1;
    let mut i = 0;
    while i < 255 {
        exp[i] = x as u8;
        log[x] = i as u8;
        x <<= 1;
        if x & 0x100 != 0 {
            x ^= 0x11d;
        }
        i += 1;
    }
    let mut j = 255;
    while j < 510 {
        exp[j] = exp[j - 255];
        j += 1;
    }
    Galois { exp, log }
}

/// GF(256) discrete log of a nonzero element.
fn gf_log(value: u8) -> usize {
    GF.log[value as usize] as usize
}

/// GF(256) multiplication.
fn gf_mul(a: u8, b: u8) -> u8 {
    if a == 0 || b == 0 {
        0
    } else {
        GF.exp[gf_log(a) + gf_log(b)]
    }
}
/// Generator polynomial for `ec_len` error codewords, **descending**
/// coefficients with a leading 1: `Π (x − α^i)` for `i` in `0..ec_len`.
/// Built ascending (constant term first, where the multiply-by-x step is
/// a simple shift) then reversed into the division-ready form.
fn rs_generator(ec_len: usize) -> Vec<u8> {
    let mut ascending: Vec<u8> = vec![1];
    for i in 0..ec_len {
        let factor = GF.exp[i];
        let mut next = vec![0u8; ascending.len() + 1];
        for (degree, slot) in next.iter_mut().enumerate() {
            // Multiply by x (shift up one degree) and by `factor`.
            let x_term = if degree >= 1 {
                ascending[degree - 1]
            } else {
                0
            };
            let a_term = if degree < ascending.len() {
                gf_mul(ascending[degree], factor)
            } else {
                0
            };
            *slot = x_term ^ a_term;
        }
        ascending = next;
    }
    ascending.reverse();
    ascending
}

/// Reed–Solomon ECC codewords for one block, via synthetic division of
/// `data · x^ec_len` by the generator polynomial.
fn rs_ecc(data: &[u8], ec_len: usize) -> Vec<u8> {
    let generator = rs_generator(ec_len);
    let mut remainder = vec![0u8; data.len() + ec_len];
    remainder[..data.len()].copy_from_slice(data);
    for i in 0..data.len() {
        let factor = remainder[i];
        if factor == 0 {
            continue;
        }
        for (j, &g) in generator.iter().enumerate() {
            remainder[i + j] ^= gf_mul(g, factor);
        }
    }
    remainder[data.len()..].to_vec()
}

/// Pushes the low `count` bits of `value`, most significant first.
fn push_bits(bits: &mut Vec<bool>, value: u32, count: u32) {
    for shift in (0..count).rev() {
        bits.push((value >> shift) & 1 == 1);
    }
}

/// Byte-mode bitstream for `payload`, padded with the standard
/// terminator and `0xEC`/`0x11` pad codewords up to the version's data
/// capacity. The caller must have checked that `payload` fits.
fn data_codewords(payload: &[u8], spec: &VersionSpec) -> Vec<u8> {
    let capacity = spec.data_codewords();
    let mut bits: Vec<bool> = Vec::with_capacity(capacity * 8);
    push_bits(&mut bits, MODE_BITS, 4);
    push_bits(&mut bits, payload.len() as u32, COUNT_BITS);
    for &byte in payload {
        push_bits(&mut bits, byte as u32, 8);
    }
    let terminator = ((capacity * 8 - bits.len()) as u32).min(4);
    push_bits(&mut bits, 0, terminator);
    while !bits.len().is_multiple_of(8) {
        bits.push(false);
    }
    let mut out = Vec::with_capacity(capacity);
    for chunk in bits.chunks(8) {
        out.push(chunk.iter().fold(0u8, |acc, &bit| (acc << 1) | bit as u8));
    }
    let mut pad_dark = true;
    while out.len() < capacity {
        out.push(if pad_dark { 0xEC } else { 0x11 });
        pad_dark = !pad_dark;
    }
    out
}

/// Splits the data codewords into blocks, computes per-block ECC, and
/// interleaves data then ECC codewords (ISO/IEC 18004 block structure).
fn interleave(data: &[u8], spec: &VersionSpec) -> Vec<u8> {
    let blocks: Vec<&[u8]> = spec
        .blocks
        .iter()
        .scan(0usize, |offset, &len| {
            let start = *offset;
            *offset += len as usize;
            Some(&data[start..*offset])
        })
        .collect();
    let eccs: Vec<Vec<u8>> = blocks
        .iter()
        .map(|block| rs_ecc(block, spec.ec_per_block as usize))
        .collect();
    let longest = spec.blocks.iter().max().map(|&b| b as usize).unwrap_or(0);
    let mut out =
        Vec::with_capacity(spec.data_codewords() + spec.blocks.len() * spec.ec_per_block as usize);
    for i in 0..longest {
        for block in &blocks {
            if let Some(&codeword) = block.get(i) {
                out.push(codeword);
            }
        }
    }
    for i in 0..spec.ec_per_block as usize {
        for ecc in &eccs {
            out.push(ecc[i]);
        }
    }
    out
}
/// An encoded QR symbol: `size × size` modules, dark = `true`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrMatrix {
    size: usize,
    modules: Vec<bool>,
}

impl QrMatrix {
    /// Edge length in modules (21 for version 1, +4 per version).
    pub fn size(&self) -> usize {
        self.size
    }

    /// Module color at (`row`, `col`); `true` is dark.
    pub fn is_dark(&self, row: usize, col: usize) -> bool {
        self.modules[row * self.size + col]
    }
}

/// Grid being drawn: module colors plus which cells belong to function
/// patterns (never receive data bits and are never masked).
struct Canvas {
    size: usize,
    modules: Vec<bool>,
    reserved: Vec<bool>,
}

impl Canvas {
    fn new(size: usize) -> Self {
        Self {
            size,
            modules: vec![false; size * size],
            reserved: vec![false; size * size],
        }
    }

    fn mark(&mut self, row: usize, col: usize, dark: bool) {
        let idx = row * self.size + col;
        self.modules[idx] = dark;
        self.reserved[idx] = true;
    }

    /// Finder pattern (7×7 rings) with its separator ring, anchored at
    /// (`row`, `col`) = the pattern's top-left module.
    fn draw_finder(&mut self, row: usize, col: usize) {
        let size = self.size;
        // Reserve the separator (one module of light on all open sides).
        for r in row.saturating_sub(1)..(row + 8).min(size) {
            for c in col.saturating_sub(1)..(col + 8).min(size) {
                let idx = r * size + c;
                self.reserved[idx] = true;
            }
        }
        for r in 0..7 {
            for c in 0..7 {
                let dark = r == 0
                    || r == 6
                    || c == 0
                    || c == 6
                    || ((2..=4).contains(&r) && (2..=4).contains(&c));
                self.mark(row + r, col + c, dark);
            }
        }
    }

    fn draw_function_patterns(&mut self, spec: &VersionSpec) {
        let size = self.size;
        self.draw_finder(0, 0);
        self.draw_finder(0, size - 7);
        self.draw_finder(size - 7, 0);
        // Timing patterns between the finders; dark on even coordinates.
        for i in 8..size - 8 {
            self.mark(6, i, i % 2 == 0);
            self.mark(i, 6, i % 2 == 0);
        }
        // Alignment patterns at center-coordinate pairs, skipping the
        // three corners that would overlap finders.
        if spec.alignment.len() > 1 {
            let last = (size - 7) as u16;
            for &r in spec.alignment {
                for &c in spec.alignment {
                    let overlaps_finder = r == 6 && (c == 6 || c == last) || r == last && c == 6;
                    if overlaps_finder {
                        continue;
                    }
                    let (r, c) = (r as usize, c as usize);
                    for dr in 0..5 {
                        for dc in 0..5 {
                            // 5×5 alignment: dark border ring, light ring,
                            // one dark center module.
                            let dark =
                                dr == 0 || dr == 4 || dc == 0 || dc == 4 || (dr == 2 && dc == 2);
                            self.mark(r - 2 + dr, c - 2 + dc, dark);
                        }
                    }
                }
            }
        }
        // Dark module and both format-info copies (bits written later,
        // after the mask is chosen, but the cells are off limits now).
        self.mark(size - 8, 8, true);
        for i in 0..9 {
            self.reserved[8 * size + i] = true;
            self.reserved[i * size + 8] = true;
            if i < 8 {
                self.reserved[8 * size + size - 1 - i] = true;
                self.reserved[(size - 1 - i) * size + 8] = true;
            }
        }
    }

    /// Zigzag data placement: two-module columns from the bottom-right
    /// upward, skipping the timing column and reserved cells.
    fn place_data(&mut self, codewords: &[u8]) {
        let size = self.size;
        let mut bit = 0usize;
        let mut col = size as isize - 1;
        let mut upward = true;
        while col > 0 {
            if col == 6 {
                col -= 1; // vertical timing pattern never carries data
            }
            for i in 0..size {
                let row = if upward { size - 1 - i } else { i };
                for c in [col, col - 1] {
                    let idx = row * size + c as usize;
                    if !self.reserved[idx] {
                        let byte = codewords.get(bit / 8).copied().unwrap_or(0);
                        self.modules[idx] = byte & (0x80 >> (bit % 8)) != 0;
                        bit += 1;
                    }
                }
            }
            upward = !upward;
            col -= 2;
        }
    }

    /// Applies mask `mask` to the data modules and writes both format-info
    /// copies (level M), producing the final symbol.
    fn finish(&self, mask: u8) -> QrMatrix {
        let size = self.size;
        let mut modules = self.modules.clone();
        for row in 0..size {
            for col in 0..size {
                let idx = row * size + col;
                if !self.reserved[idx] && mask_bit(mask, row, col) {
                    modules[idx] = !modules[idx];
                }
            }
        }
        let bits = format_bits(mask);
        // Copy 1 (top-left): row 8 cols 0..5, then (8,7) (8,8) (7,8) and
        // col 8 rows 5..0 — column six belongs to the timing pattern.
        for i in 0..6 {
            modules[8 * size + i] = bit_at(bits, 14 - i);
            modules[(5 - i) * size + 8] = bit_at(bits, 5 - i);
        }
        modules[8 * size + 7] = bit_at(bits, 8);
        modules[8 * size + 8] = bit_at(bits, 7);
        modules[7 * size + 8] = bit_at(bits, 6);
        // Copy 2: col 8 rows size-1..size-7, then row 8 cols size-8..size-1.
        for i in 0..7 {
            modules[(size - 1 - i) * size + 8] = bit_at(bits, 14 - i);
        }
        for i in 0..8 {
            modules[8 * size + (size - 8 + i)] = bit_at(bits, 7 - i);
        }
        QrMatrix { size, modules }
    }
}

/// `true` when mask condition `mask` fires at (`row`, `col`).
fn mask_bit(mask: u8, row: usize, col: usize) -> bool {
    let (r, c) = (row as u64, col as u64);
    match mask {
        0 => (r + c) % 2 == 0,
        1 => r % 2 == 0,
        2 => c % 2 == 0,
        3 => (r + c) % 3 == 0,
        4 => (r / 2 + c / 3) % 2 == 0,
        5 => (r * c) % 2 + (r * c) % 3 == 0,
        6 => ((r * c) % 2 + (r * c) % 3) % 2 == 0,
        _ => ((r + c) % 2 + (r * c) % 3) % 2 == 0,
    }
}

/// 15-bit format information for level M and mask `mask`:
/// 5 data bits, 10 BCH parity bits, XORed with the constant 0x5412.
fn format_bits(mask: u8) -> u16 {
    let data = mask as u16; // level M contributes `00` in bits 4..3
    let mut rem = data << 10;
    for shift in (10..15).rev() {
        if (rem >> shift) & 1 == 1 {
            rem ^= 0x537 << (shift - 10);
        }
    }
    ((data << 10) | rem) ^ 0x5412
}

/// Bit `n` of `value` (bit 14 = most significant format bit).
fn bit_at(value: u16, n: usize) -> bool {
    (value >> n) & 1 == 1
}

/// Mask-penalty score (ISO/IEC 18004 §8.8.2): runs, 2×2 blocks,
/// finder-like patterns and the dark/light balance, all four rules.
fn penalty(modules: &[bool], size: usize) -> u32 {
    let mut score = 0u32;
    // N1: five-plus runs of one color, rows and columns.
    for row in 0..size {
        let mut run_color = modules[row * size];
        let mut run_len = 1usize;
        for col in 1..size {
            let dark = modules[row * size + col];
            if dark == run_color {
                run_len += 1;
            } else {
                if run_len >= 5 {
                    score += 3 + (run_len - 5) as u32;
                }
                run_color = dark;
                run_len = 1;
            }
        }
        if run_len >= 5 {
            score += 3 + (run_len - 5) as u32;
        }
    }
    for col in 0..size {
        let mut run_color = modules[col];
        let mut run_len = 1usize;
        for row in 1..size {
            let dark = modules[row * size + col];
            if dark == run_color {
                run_len += 1;
            } else {
                if run_len >= 5 {
                    score += 3 + (run_len - 5) as u32;
                }
                run_color = dark;
                run_len = 1;
            }
        }
        if run_len >= 5 {
            score += 3 + (run_len - 5) as u32;
        }
    }
    // N2: every 2×2 block of one color.
    for row in 0..size - 1 {
        for col in 0..size - 1 {
            let a = modules[row * size + col];
            if a == modules[row * size + col + 1]
                && a == modules[(row + 1) * size + col]
                && a == modules[(row + 1) * size + col + 1]
            {
                score += 3;
            }
        }
    }
    // N3: `1011101` with four light modules on either side, rows and cols.
    for row in 0..size {
        for col in 0..size.saturating_sub(10) {
            let window: Vec<bool> = (0..11).map(|i| modules[row * size + col + i]).collect();
            if finder_like(&window) {
                score += 40;
            }
        }
    }
    for col in 0..size {
        for row in 0..size.saturating_sub(10) {
            let window: Vec<bool> = (0..11).map(|i| modules[(row + i) * size + col]).collect();
            if finder_like(&window) {
                score += 40;
            }
        }
    }
    // N4: dark-ratio deviation from 50%, 10 points per 5% step.
    let dark = modules.iter().filter(|m| **m).count() as u32;
    let total = (size * size) as u32;
    let ratio = dark * 100 / total;
    score += ratio.abs_diff(50) / 5 * 10;
    score
}

/// `00001011101` or `10111010000`.
fn finder_like(window: &[bool]) -> bool {
    let core = [true, false, true, true, true, false, true];
    let light = [false; 4];
    window[..4] == light && window[4..] == core || window[..7] == core && window[7..] == light
}

/// Encodes `payload` at the smallest supported version (byte mode, level
/// M), choosing the mask with the lowest penalty score.
pub fn encode(payload: &[u8]) -> Result<QrMatrix, QrTooLarge> {
    let (spec, codewords) = prepare(payload)?;
    let size = 17 + 4 * spec.version as usize;
    let mut canvas = Canvas::new(size);
    canvas.draw_function_patterns(spec);
    canvas.place_data(&codewords);
    let mut best: Option<(QrMatrix, u32)> = None;
    for mask in 0..8u8 {
        let candidate = canvas.finish(mask);
        let score = penalty(&candidate.modules, size);
        if best
            .as_ref()
            .is_none_or(|(_, best_score)| score < *best_score)
        {
            best = Some((candidate, score));
        }
    }
    Ok(best.map_or_else(|| canvas.finish(0), |(matrix, _)| matrix))
}

/// Version selection + interleaved codewords.
fn prepare(payload: &[u8]) -> Result<(&'static VersionSpec, Vec<u8>), QrTooLarge> {
    let spec = VERSIONS
        .iter()
        .find(|spec| payload.len() <= spec.byte_capacity())
        .ok_or(QrTooLarge {
            needed: payload.len(),
            max: VERSIONS[5].byte_capacity(),
        })?;
    let codewords = interleave(&data_codewords(payload, spec), spec);
    Ok((spec, codewords))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn galois_tables_round_trip() {
        for value in 1..=255u8 {
            assert_eq!(GF.exp[gf_log(value)], value);
        }
        for i in 0..255 {
            assert_eq!(GF.log[GF.exp[i] as usize] as usize, i);
        }
        // Double-length exp copy keeps multiplication wrap-free.
        assert_eq!(gf_mul(GF.exp[100], GF.exp[200]), GF.exp[300 % 255]);
    }

    #[test]
    fn tiny_payload_bitstream_is_exact() {
        // "A" = 0x41: mode 0100, count 00000001, byte 01000001,
        // 4-bit terminator, then 0xEC/0x11 pad — 16 codewords at v1-M.
        let codewords = data_codewords(b"A", &VERSIONS[0]);
        assert_eq!(codewords.len(), 16);
        assert_eq!(&codewords[..3], &[0x40, 0x14, 0x10]);
        for (i, &word) in codewords[3..].iter().enumerate() {
            assert_eq!(word, if i % 2 == 0 { 0xEC } else { 0x11 });
        }
    }

    #[test]
    fn picks_smallest_fitting_version() {
        let spec_for = |payload: &[u8]| {
            VERSIONS
                .iter()
                .find(|spec| payload.len() <= spec.byte_capacity())
                .map(|spec| spec.version)
        };
        assert_eq!(spec_for(&[0; 14]), Some(1));
        assert_eq!(spec_for(&[0; 15]), Some(2));
        assert_eq!(spec_for(&[0; 26]), Some(2));
        assert_eq!(spec_for(&[0; 27]), Some(3));
        assert_eq!(spec_for(&[0; 62]), Some(4));
        assert_eq!(spec_for(&[0; 84]), Some(5));
        assert_eq!(spec_for(&[0; 106]), Some(6));
        let err = encode(&[0; 107]).unwrap_err();
        assert_eq!(
            err,
            QrTooLarge {
                needed: 107,
                max: 106
            }
        );
    }

    #[test]
    fn rs_codewords_have_vanishing_syndromes() {
        // For every supported version: a block plus its ECC must be a
        // valid codeword — the polynomial evaluates to zero at α^i for
        // every syndrome i. Horner evaluation is an independent check of
        // the generator + division pair.
        let mut seed = 0x2Fu8;
        let mut data_byte = || {
            seed = seed.rotate_left(3) ^ 0x5A;
            seed
        };
        for spec in VERSIONS {
            let data: Vec<u8> = (0..spec.data_codewords()).map(|_| data_byte()).collect();
            let ecc = rs_ecc(&data, spec.ec_per_block as usize);
            let mut codeword = data;
            codeword.extend(ecc);
            for i in 0..spec.ec_per_block as usize {
                let alpha = GF.exp[i];
                let mut acc = 0u8;
                for &coef in &codeword {
                    acc = gf_mul(acc, alpha) ^ coef;
                }
                assert_eq!(acc, 0, "syndrome {i} nonzero at version {}", spec.version);
            }
        }
    }

    #[test]
    fn interleave_alternates_blocks() {
        // Version 4-M: two blocks of 32. Data codewords interleave 1,1',
        // 2,2'…; ECC follows the same round-robin.
        let data: Vec<u8> = (0..64u8).collect();
        let out = interleave(&data, &VERSIONS[3]);
        assert_eq!(out.len(), 64 + 2 * 18);
        assert_eq!(&out[..4], &[0, 32, 1, 33]);
        let first_block_ecc = rs_ecc(&data[..32], 18);
        assert_eq!(out[64], first_block_ecc[0]);
        assert_eq!(out[65], rs_ecc(&data[32..], 18)[0]);
    }

    #[test]
    fn format_bits_for_zero_mask_is_the_known_constant() {
        // Level M (00) + mask 0 → all-zero BCH payload XORed with 0x5412.
        assert_eq!(format_bits(0), 0x5412);
        // Every mask survives its own BCH check once unmasked.
        for mask in 0..8u8 {
            let unmasked = format_bits(mask) ^ 0x5412;
            assert_eq!(unmasked >> 10, mask as u16);
            let mut rem = unmasked;
            for shift in (10..15u32).rev() {
                if (rem >> shift) & 1 == 1 {
                    rem ^= 0x537 << (shift - 10);
                }
            }
            assert_eq!(rem, 0, "BCH parity broken for mask {mask}");
        }
    }

    #[test]
    fn matrix_carries_required_structure() {
        let matrix = encode(b"pairing-code-structure-test-0123456789").unwrap();
        // 34 bytes → version 3 → 29×29.
        assert_eq!(matrix.size(), 29);
        // Finder rings: outer border dark, one-in light, 3×3 center dark;
        // the separator module just outside each finder stays light.
        for (row, col, sep_row, sep_col) in [
            (0usize, 0usize, 3usize, 7usize),
            (0, 22, 3, 21),
            (22, 0, 25, 7),
        ] {
            assert!(matrix.is_dark(row, col));
            assert!(!matrix.is_dark(row + 1, col + 1));
            assert!(matrix.is_dark(row + 3, col + 3));
            assert!(!matrix.is_dark(sep_row, sep_col));
        }
        // Timing row six alternates light on odd columns inside the span.
        assert!(!matrix.is_dark(6, 9));
        assert!(matrix.is_dark(6, 10));
        // Alignment pattern centered at (22, 22) for version 3.
        assert!(matrix.is_dark(20, 20));
        assert!(!matrix.is_dark(21, 21));
        assert!(matrix.is_dark(22, 22));
        // Dark module at (size − 8, 8).
        assert!(matrix.is_dark(matrix.size() - 8, 8));
    }

    #[test]
    fn format_copies_match_and_decode() {
        let matrix = encode(b"copy-check").unwrap();
        let size = matrix.size();
        // Copy 1.
        let mut copy1 = Vec::new();
        for col in 0..6 {
            copy1.push(matrix.is_dark(8, col));
        }
        copy1.push(matrix.is_dark(8, 7));
        copy1.push(matrix.is_dark(8, 8));
        copy1.push(matrix.is_dark(7, 8));
        for row in (0..6).rev() {
            copy1.push(matrix.is_dark(row, 8));
        }
        // Copy 2.
        let mut copy2 = Vec::new();
        for row in 0..7 {
            copy2.push(matrix.is_dark(size - 1 - row, 8));
        }
        for col in 0..8 {
            copy2.push(matrix.is_dark(8, size - 8 + col));
        }
        // Both copies are 15 bits, bit 14 first.
        let to_u16 = |bits: &[bool]| bits.iter().fold(0u16, |acc, &bit| (acc << 1) | bit as u16);
        let (a, b) = (to_u16(&copy1), to_u16(&copy2));
        assert_eq!(a, b, "the two format-info copies disagree");
        let unmasked = a ^ 0x5412;
        // Level bits must read 00 (level M) once unmasked.
        assert_eq!(unmasked >> 13, 0);
    }

    #[test]
    fn encoding_is_deterministic() {
        let payload = b"android18-pairing-determinism";
        assert_eq!(encode(payload).unwrap(), encode(payload).unwrap());
    }

    #[test]
    fn pairing_sized_payload_fits() {
        let json = br#"{"v":1,"ip":"192.168.100.100","port":54321,"code":"a1b2c3d4e5f6"}"#;
        let matrix = encode(json).expect("pairing JSON fits version 5-M");
        assert_eq!(matrix.size(), 37); // 64 bytes → version 5 → 37 modules
    }
}
