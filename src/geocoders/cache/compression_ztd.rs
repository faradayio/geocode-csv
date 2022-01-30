// //! Basic compression and decompression for cached data, using a custom language
// //! model to build a dictionary for `zstd`.

// use std::cmp::min;

// use anyhow::Context;
// use metrics::counter;
// use zstd::{
//     bulk::{Compressor, Decompressor},
//     dict::{DecoderDictionary, EncoderDictionary},
//     DEFAULT_COMPRESSION_LEVEL,
// };

// use crate::Result;

// /// The pre-built dictionary from `build.rs`, using the various CSV files in
// /// this directory.
// static DICT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/zstd_dict.bin"));

// /// Interface for compressing and decompressing cache entries.
// pub struct CacheCompressor {
//     /// Compression dictionary.
//     endict: EncoderDictionary<'static>,

//     /// Decompression dictionary.
//     dedict: DecoderDictionary<'static>,
// }

// impl CacheCompressor {
//     /// Create a new cache compressor and initialize our dictionaries.
//     pub fn new() -> CacheCompressor {
//         let endict = EncoderDictionary::copy(DICT, 20);
//         let dedict = DecoderDictionary::copy(DICT);
//         CacheCompressor { endict, dedict }
//     }

//     /// Compress `input` and store in `output`.
//     pub fn compress(&self, input: &[u8]) -> Result<Vec<u8>> {
//         counter!("zstd.compress.in", input.len() as u64);
//         let mut compressor = Compressor::with_prepared_dictionary(&self.endict)
//             .context("could not create compressor")?;
//         let output = compressor
//             .compress(input)
//             .context("could not compress data")?;
//         counter!("zstd.compress.out", output.len() as u64);
//         Ok(output)
//     }

//     /// Decompress `input` and store in `output`.
//     pub fn decompress(&self, input: &[u8]) -> Result<Vec<u8>> {
//         counter!("zstd.decompress.in", input.len() as u64);
//         let mut decompressor = Decompressor::with_prepared_dictionary(&self.dedict)
//             .context("could not create decompressor")?;
//         let output = decompressor
//             // TODO: Why do we not have decompression bounds?
//             .decompress(input, min(input.len() * 20, 512))
//             .context("could not decompress data")?;
//         counter!("zstd.decompress.out", output.len() as u64);
//         Ok(output)
//     }
// }

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn round_trip_compression() {
//         let examples = &[
//             "781 Franklin Ave Crown Heights Brooklyn NYC NY 11216 USA",
//             "20 W 34th St, New York, NY 10001",
//             "",
//             "abc123",
//         ];

//         let cache_compressor = CacheCompressor::new();
//         for &example in examples {
//             let compressed = cache_compressor.compress(example.as_bytes()).unwrap();
//             let decompressed = cache_compressor.decompress(&compressed).unwrap();
//             assert_eq!(example.as_bytes(), decompressed);
//         }
//     }
// }
