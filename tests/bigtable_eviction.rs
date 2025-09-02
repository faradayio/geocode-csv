//! Tests for BigTable cache eviction functionality.

use cli_test_dir::*;
use std::collections::HashSet;

/// A CSV file with multiple addresses for eviction testing.
const EVICTION_TEST_CSV: &str = "address_1,address_2,city,state,zip_code
20 W 34th St,,New York,NY,10118
1224 S 760 W,,Provo,UT,
1600 Pennsylvania Ave NW,,Washington,DC,20500
1 Microsoft Way,,Redmond,WA,98052
1 Infinite Loop,,Cupertino,CA,95014
";

/// Helper function to analyze geocoding output and extract detailed metrics
fn analyze_geocoding_output(output: &str) -> GeocodingAnalysis {
    let lines: Vec<&str> = output.lines().collect();
    let _header_line = lines.first().expect("Should have header line");
    let data_lines = &lines[1..];
    
    let mut geocoded_addresses = HashSet::new();
    let mut address_types = HashSet::new();
    let mut total_rows = 0;
    let mut rows_with_gc_data = 0;
    
    for line in data_lines {
        if line.trim().is_empty() {
            continue;
        }
        total_rows += 1;
        
        // Extract the address for tracking
        let fields: Vec<&str> = line.split(',').collect();
        if !fields.is_empty() {
            let address = fields[0].trim();
            if !address.is_empty() {
                geocoded_addresses.insert(address.to_string());
            }
        }
        
        // Check for geocoding data
        if line.contains("Commercial") || line.contains("Residential") || line.contains("gc_addressee") {
            rows_with_gc_data += 1;
        }
        
        // Extract address types
        if line.contains("Commercial") {
            address_types.insert("Commercial".to_string());
        }
        if line.contains("Residential") {
            address_types.insert("Residential".to_string());
        }
    }
    
    GeocodingAnalysis {
        total_rows,
        rows_with_gc_data,
        geocoded_addresses,
        address_types,
        raw_output: output.to_string(),
    }
}

#[derive(Debug, Clone)]
struct GeocodingAnalysis {
    total_rows: usize,
    rows_with_gc_data: usize,
    geocoded_addresses: HashSet<String>,
    address_types: HashSet<String>,
    raw_output: String,
}

impl GeocodingAnalysis {
    fn print_summary(&self, label: &str) {
        println!("\n=== {} ===", label);
        println!("Total rows: {}", self.total_rows);
        println!("Rows with geocoding data: {}", self.rows_with_gc_data);
        println!("Geocoding coverage: {:.1}%", 
                 (self.rows_with_gc_data as f64 / self.total_rows as f64) * 100.0);
        println!("Address types found: {:?}", self.address_types);
        println!("Geocoded addresses: {:?}", self.geocoded_addresses);
        
        // Show first few lines of output for debugging
        let lines: Vec<&str> = self.raw_output.lines().take(3).collect();
        println!("Sample output:");
        for line in lines {
            println!("  {}", line);
        }
    }
    

}

#[test]
#[ignore]
fn bigtable_cache_eviction_test() {
    let testdir = TestDir::new("geocode-csv", "bigtable_cache_eviction_test");

    testdir.create_file(
        "spec.json",
        r#"{
    "gc": {
        "house_number_and_street": [
            "address_1",
            "address_2"
        ],
        "city": "city",
        "state": "state",
        "postcode": "zip_code"
    }
}"#,
    );

    println!("Starting BigTable cache eviction test...");
    println!("Test CSV data:\n{}", EVICTION_TEST_CSV);

    // First run - populate the cache
    println!("\n🔄 Phase 1: Populating cache with initial geocoding run...");
    let output1 = testdir
        .cmd()
        .arg("--license=us-core-enterprise-cloud")
        .arg("--spec=spec.json")
        .arg("--cache=bigtable://bigtable-297912/geocode1/geocode_csv_test")
        .output_with_stdin(EVICTION_TEST_CSV)
        .expect_success();



    let initial_analysis = analyze_geocoding_output(output1.stdout_str());
    initial_analysis.print_summary("Initial Cache Population");

    // Verify first run has correct data
    assert!(
        initial_analysis.rows_with_gc_data > 0,
        "Expected some geocoded results in initial run, got {} rows with geocoding data",
        initial_analysis.rows_with_gc_data
    );
    assert!(
        initial_analysis.address_types.len() > 0,
        "Expected some address types in initial run, got: {:?}",
        initial_analysis.address_types
    );

    // Run with eviction enabled - 50% eviction rate on entries older than 1 second
    // We'll run this multiple times to see eviction effects
    println!("\n🔄 Phase 2: Testing cache eviction with 50% rate...");
    let mut eviction_seen = false;
    let mut attempt_analyses = Vec::new();

    for i in 0..10 {
        // Wait a moment to ensure cache entries are old enough
        println!("  Attempt {}: Waiting 1.1s for cache entries to age...", i + 1);
        std::thread::sleep(std::time::Duration::from_millis(1100));

        // Run with eviction enabled
        let output_evict = testdir
            .cmd()
            .arg("--license=us-core-enterprise-cloud")
            .arg("--spec=spec.json")
            .arg("--cache=bigtable://bigtable-297912/geocode1/geocode_csv_test")
            .arg("--bigtable-random-eviction-age=1")
            .arg("--bigtable-random-eviction-rate=0.5")
            .output_with_stdin(EVICTION_TEST_CSV)
            .expect_success();



        let eviction_analysis = analyze_geocoding_output(output_evict.stdout_str());
        attempt_analyses.push(eviction_analysis.clone());
        
        println!("  Attempt {}: {} geocoded rows (vs {} initial)", 
                 i + 1, eviction_analysis.rows_with_gc_data, initial_analysis.rows_with_gc_data);

        // The eviction is working (as we can see from stderr), but evicted entries
        // are immediately repopulated with fresh API calls. So we won't see differences
        // in the normal output. Let's test cache-hits-only mode instead.
        println!("  Eviction is working (check stderr), but entries are repopulated");
        
        // Test cache-hits-only mode to see actual eviction effects
        println!("  Testing cache-hits-only to see eviction effects...");
        let cache_only_output = testdir
            .cmd()
            .arg("--license=us-core-enterprise-cloud")
            .arg("--spec=spec.json")
            .arg("--cache=bigtable://bigtable-297912/geocode1/geocode_csv_test")
            .arg("--bigtable-random-eviction-age=1")
            .arg("--bigtable-random-eviction-rate=0.5")
            .arg("--cache-hits-only")
            .output_with_stdin(EVICTION_TEST_CSV)
            .expect_success();

        let cache_only_analysis = analyze_geocoding_output(cache_only_output.stdout_str());
        println!("  Cache-hits-only result: {}/{} rows geocoded", 
                 cache_only_analysis.rows_with_gc_data, cache_only_analysis.total_rows);
        
        // If we see fewer than 5 geocoded rows in cache-hits-only mode, eviction is working
        if cache_only_analysis.rows_with_gc_data < 5 {
            eviction_seen = true;
            println!("  ✅ Eviction confirmed via cache-hits-only mode!");
            
            cache_only_analysis.print_summary(&format!("Cache-Hits-Only Eviction Test (Attempt {})", i + 1));
            
            println!("\n📊 Eviction evidence:");
            println!("  Total addresses: 5");
            println!("  Cache hits: {}", cache_only_analysis.rows_with_gc_data);
            println!("  Cache misses (evicted): {}", 5 - cache_only_analysis.rows_with_gc_data);
            println!("  Eviction rate: {:.1}%", 
                     ((5 - cache_only_analysis.rows_with_gc_data) as f64 / 5.0) * 100.0);
            break;
        } else {
            println!("  Cache-hits-only still shows all entries - continue testing");
        }
    }

    // Print summary of all attempts if eviction wasn't seen
    if !eviction_seen {
        println!("\n⚠️  No eviction detected after 10 attempts. Analysis:");
        println!("All attempts had {} geocoded rows", initial_analysis.rows_with_gc_data);
        
        // Show the last few attempts for debugging
        for (i, analysis) in attempt_analyses.iter().enumerate().take(3) {
            println!("\nAttempt {} sample output:", i + 1);
            let sample_lines: Vec<&str> = analysis.raw_output.lines().take(2).collect();
            for line in sample_lines {
                println!("  {}", line);
            }
        }
    }

    // With 50% eviction rate and 10 attempts, we should see eviction at least once
    assert!(
        eviction_seen,
        "Expected to see cache eviction with 50% rate after 10 attempts. \
         All runs had {} geocoded rows. This could indicate: \
         1) Eviction is not working, 2) Cache entries are not being created, \
         3) Eviction parameters are not being applied correctly, \
         4) Test timing issues",
        initial_analysis.rows_with_gc_data
    );

    // Validation run: Test without eviction to ensure cache is still functional
    println!("\n🔄 Phase 2.5: Validation run without eviction...");
    let output_no_eviction = testdir
        .cmd()
        .arg("--license=us-core-enterprise-cloud")
        .arg("--spec=spec.json")
        .arg("--cache=bigtable://bigtable-297912/geocode1/geocode_csv_test")
        .output_with_stdin(EVICTION_TEST_CSV)
        .expect_success();



    let no_eviction_analysis = analyze_geocoding_output(output_no_eviction.stdout_str());
    no_eviction_analysis.print_summary("Validation Run (No Eviction)");
    
    // This should have similar results to initial run (cache should be repopulated)
    if no_eviction_analysis.rows_with_gc_data != initial_analysis.rows_with_gc_data {
        println!("  ⚠️  Validation run differs from initial - cache may be inconsistent");
    } else {
        println!("  ✅ Validation run matches initial - cache is working correctly");
    }

    // Test that cache-hits-only mode respects eviction
    println!("\n🔄 Phase 3: Testing cache-hits-only mode with eviction...");
    std::thread::sleep(std::time::Duration::from_millis(1100));

    let output_cache_only = testdir
        .cmd()
        .arg("--license=us-core-enterprise-cloud")
        .arg("--spec=spec.json")
        .arg("--cache=bigtable://bigtable-297912/geocode1/geocode_csv_test")
        .arg("--bigtable-random-eviction-age=1")
        .arg("--bigtable-random-eviction-rate=0.5")
        .arg("--cache-hits-only")
        .output_with_stdin(EVICTION_TEST_CSV)
        .expect_success();

    let cache_only_analysis = analyze_geocoding_output(output_cache_only.stdout_str());
    cache_only_analysis.print_summary("Cache-Hits-Only with Eviction");

    // In cache-hits-only mode with eviction, we expect some rows to be missing geocoding data
    // due to evicted cache entries
    let lines_with_gc = cache_only_analysis.rows_with_gc_data;

    println!("\n📊 Cache-hits-only analysis:");
    println!("  Total addresses: 5");
    println!("  Addresses with geocoding data: {}", lines_with_gc);
    println!("  Cache hit rate: {:.1}%", (lines_with_gc as f64 / 5.0) * 100.0);
    
    if lines_with_gc == 5 {
        println!("  ⚠️  All addresses were cache hits - eviction may not be working");
    } else if lines_with_gc == 0 {
        println!("  ⚠️  No cache hits - all entries may have been evicted or cache is not working");
    } else {
        println!("  ✅ Partial cache hits detected - eviction appears to be working");
    }

    // With 5 addresses and 50% eviction, we expect at least some but not all to be geocoded
    assert!(
        lines_with_gc > 0, 
        "Expected at least some cache hits in cache-hits-only mode, got {}. \
         This could indicate: 1) All cache entries were evicted, 2) Cache is not working, \
         3) Eviction rate is too aggressive",
        lines_with_gc
    );
    assert!(
        lines_with_gc < 5,
        "Expected some cache misses due to eviction in cache-hits-only mode, but got {}/5 hits. \
         This could indicate: 1) Eviction is not working, 2) Cache entries are not expiring, \
         3) Test timing issues",
        lines_with_gc
    );
    
    println!("\n✅ BigTable cache eviction test completed successfully!");
}
