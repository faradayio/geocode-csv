//! Basic compression and decompression for cached data, using a custom language
//! model to build a dictionary for `zstd`.

use metrics::{counter, describe_counter, Unit};

use crate::Result;

/// Interface for compressing and decompressing cache entries.
///
/// We want to allow alternate implementations in the future.
pub struct CacheCompressor {}

impl CacheCompressor {
    /// Create a new cache compressor and initialize our dictionaries.
    pub fn new() -> CacheCompressor {
        describe_counter!(
            "geocodecsv.compressor_input.bytes_total",
            Unit::Bytes,
            "Bytes input to compressor"
        );
        describe_counter!(
            "geocodecsv.compressor_output.bytes_total",
            Unit::Bytes,
            "Bytes output by compressor"
        );
        describe_counter!(
            "geocodecsv.decompressor_input.bytes_total",
            Unit::Bytes,
            "Bytes input to decompressor"
        );
        describe_counter!(
            "geocodecsv.decompressor_output.bytes_total",
            Unit::Bytes,
            "Bytes output by decompressor"
        );

        CacheCompressor {}
    }

    /// The ID of this type of compression. Must be unique.
    pub fn id(&self) -> u8 {
        b'N' // "N" means "none".
    }

    /// Compress `input` and append to `output`.
    pub fn compress(&self, input: &[u8], output: &mut Vec<u8>) -> Result<()> {
        counter!("geocodecsv.compressor_input.bytes_total", input.len() as u64, "compressor" => "none");
        output.extend_from_slice(input);
        counter!("geocodecsv.compressor_output.bytes_total", output.len() as u64, "compressor" => "none");
        Ok(())
    }

    /// Decompress `input` and store in `output`.
    pub fn decompress(&self, input: &[u8], output: &mut Vec<u8>) -> Result<()> {
        counter!("geocodecsv.decompressor_input.bytes_total", input.len() as u64, "compressor" => "none");
        output.extend_from_slice(input);
        counter!("geocodecsv.decompressor_output.bytes_total", output.len() as u64, "compressor" => "none");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_compression() {
        let examples: &[&[u8]] = &[
            b"781 Franklin Ave Crown Heights Brooklyn NYC NY 11216 USA",
            b"20 W 34th St, New York, NY 10001",
            b"",
            b"abc123",
        ];

        let cache_compressor = CacheCompressor::new();
        for &example in examples {
            let mut compressed = vec![];
            cache_compressor.compress(example, &mut compressed).unwrap();
            let mut decompressed = vec![];
            cache_compressor
                .decompress(&compressed, &mut decompressed)
                .unwrap();
            assert_eq!(example, decompressed);
        }
    }
}
