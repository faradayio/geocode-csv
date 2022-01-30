// use std::{
//     cmp::min,
//     collections::HashMap,
//     env, fs,
//     path::{Path, PathBuf},
// };

fn main() {
    // build_zstd_compression_dictionary();
}

// /// If a word does not appear at least this many times, we won't bother
// /// compressing it.
// const DICTIONARY_MIN_WORD_COUNT: u64 = 10_000;

// /// Build a compression dictionary that we can pass to `zstd` for compression
// /// short address documents efficiently.
// ///
// /// WARNING: If you mess with this _at all_, you may break decompression of
// /// existing cache entries. So you'll probably wind up needing to keep around
// /// both the old and the new dictionary.
// fn build_zstd_compression_dictionary() {
//     // "Training" data for our compression dictionary, from several sources,
//     // with frequency counts.
//     let weighted_files = &[
//         // Common words in addresses.
//         (1u64, Path::new("src/geocoders/cache/address_words.csv")),
//         // Common "enum" words that appear in API output. We weight these higher
//         // because we used only 1/40th the number of total records.
//         (40u64, Path::new("src/geocoders/cache/enum_words_1.csv")),
//     ];

//     // Read our word counts from our input files and weight them.
//     let mut word_counts: HashMap<String, u64> = HashMap::new();
//     for (weight, path) in weighted_files.iter().cloned() {
//         println!("cargo:rerun-if-changed={}", path.display());

//         let mut rdr = csv::Reader::from_path(path).expect("could not open");
//         for result in rdr.deserialize::<(u64, String)>() {
//             let (cnt, word) = result.expect("could not read");
//             *word_counts.entry(word).or_default() += cnt * weight;
//         }
//     }

//     // Sort our word counts in descending order, with a cutoff below a certain
//     // frequency.
//     let mut word_counts = word_counts
//         .into_iter()
//         .filter(|(_w, c)| *c >= DICTIONARY_MIN_WORD_COUNT)
//         .collect::<Vec<_>>();
//     word_counts.sort_by(|(_w1, c1), (_w2, c2)| c1.cmp(c2).reverse());

//     // Build a training vector with all our words, and their ASCII lowercase
//     // variants.
//     let mut training = vec![];
//     for (word, count) in word_counts {
//         let repeats = min(count / DICTIONARY_MIN_WORD_COUNT, 1);
//         for _ in 0..repeats {
//             training.push(word.clone());
//             // I'm not sure if this is a net win or not.
//             training.push(word.to_ascii_lowercase());
//         }
//     }

//     // Build our dictionary.
//     let dict = zstd::dict::from_samples(&training, 128 * 1024)
//         .expect("could not build dictionary");

//     // Write our dictionary to disk.
//     let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
//     fs::write(out_path.join("zstd_dict.bin"), &dict)
//         .expect("Couldn't write bindings!");
// }
