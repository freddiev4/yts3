use std::f64::consts::PI;

use crate::config;

/// Precomputed 8x8 DCT basis functions for embedding and extraction.
pub struct DctTables {
    /// For each possible bit value (0 or 1), the 8x8 pixel block pattern.
    pub embed_blocks: [[u8; 64]; 2],
    /// Projection vector for extracting a single bit from an 8x8 block via dot product.
    pub projection: [f64; 64],
}

impl DctTables {
    pub fn new(coefficient_strength: f64) -> Self {
        // Compute the DCT basis vectors for the embed positions
        let mut projection = [0.0f64; 64];
        let mut embed_pattern = [0.0f64; 64]; // pattern for bit=1

        for &(u, v) in &config::EMBED_POSITIONS {
            let basis = dct_basis(u, v);
            for i in 0..64 {
                projection[i] += basis[i];
                embed_pattern[i] += basis[i];
            }
        }

        // Normalize projection
        let norm: f64 = projection.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm > 0.0 {
            for p in projection.iter_mut() {
                *p /= norm;
            }
        }

        // Generate the two block patterns (bit=0 and bit=1)
        let mut block_0 = [0u8; 64];
        let mut block_1 = [0u8; 64];

        for i in 0..64 {
            // Baseline is 128 (mid-gray)
            let val_0 = 128.0 - coefficient_strength * embed_pattern[i];
            let val_1 = 128.0 + coefficient_strength * embed_pattern[i];
            block_0[i] = val_0.clamp(0.0, 255.0) as u8;
            block_1[i] = val_1.clamp(0.0, 255.0) as u8;
        }

        Self {
            embed_blocks: [block_0, block_1],
            projection,
        }
    }

    /// Extract a single bit from an 8x8 block using the projection vector.
    pub fn extract_bit(&self, block: &[u8; 64]) -> u8 {
        let dot: f64 = block
            .iter()
            .zip(self.projection.iter())
            .map(|(&pixel, &proj)| (pixel as f64 - 128.0) * proj)
            .sum();

        if dot > 0.0 { 1 } else { 0 }
    }
}

/// Compute the 8x8 DCT-II basis function for frequency indices (u, v).
/// Returns a flattened 64-element array representing the 8x8 block.
fn dct_basis(u: usize, v: usize) -> [f64; 64] {
    let mut basis = [0.0f64; 64];
    let cu = if u == 0 {
        1.0 / (2.0_f64).sqrt()
    } else {
        1.0
    };
    let cv = if v == 0 {
        1.0 / (2.0_f64).sqrt()
    } else {
        1.0
    };

    for y in 0..8 {
        for x in 0..8 {
            let cos_x = ((2 * x + 1) as f64 * u as f64 * PI / 16.0).cos();
            let cos_y = ((2 * y + 1) as f64 * v as f64 * PI / 16.0).cos();
            basis[y * 8 + x] = 0.25 * cu * cv * cos_x * cos_y;
        }
    }

    basis
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embed_extract_roundtrip() {
        let tables = DctTables::new(config::DEFAULT_COEFFICIENT_STRENGTH);

        // Bit 0
        let mut block_0 = [0u8; 64];
        block_0.copy_from_slice(&tables.embed_blocks[0]);
        assert_eq!(tables.extract_bit(&block_0), 0);

        // Bit 1
        let mut block_1 = [0u8; 64];
        block_1.copy_from_slice(&tables.embed_blocks[1]);
        assert_eq!(tables.extract_bit(&block_1), 1);
    }

    #[test]
    fn test_dct_basis_dc() {
        let basis = dct_basis(0, 0);
        // DC component should be constant across all pixels
        let first = basis[0];
        for &val in &basis[1..] {
            assert!((val - first).abs() < 1e-10);
        }
    }
}
