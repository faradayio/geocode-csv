# `geocode-csv`: Geocode a CSV file using libpostal or the Smarty API

(This project is not associated with [Smarty][].)

**WARNING: This project geocodes CSV files thousands of rows per second, which can use up your Smarty quota very quickly.** This may cost you money.

If you have a CSV file that appears as follows:

```csv
name,street1,street2,city,state,zip
Resident,1600 Pennsylvania Avenue NW,,Washington DC,20500
```

...and an `address_spec.json` file that appears as follows:

```json
{
  "geocoded": {
    "street": ["street1", "street2"],
    "city": "city",
    "state": "state",
    "zipcode": "zip"
  }
}
```

...then you can geocode it using:

```sh
# Set up credentials.
export SMARTY_AUTH_ID=...
export SMARTY_AUTH_TOKEN=...

# Geocode the CSV.
geocode-csv --spec address_spec.json < in.csv > out.csv
```

This will add a series of columns starting with `geocoded_`, which will contain various postal delivery information, plus estimated latitude and longitude. If geocoding succeeds, `geocode-csv` will return 0. If it fails, it will return a non-zero error code and print a human-readable error message to standard error.

You can geocode multiple addresses per row as follows:

```json
{
  "geocoded_shipping": {
    /* ... */
  },
  "geocoded_billing": {
    /* ... */
  }
}
```

This will insert two sets of columns, one beginning with `geocoded_shipping_` and the other with `geocoded_billing_`.

## Caching

Pass `--cache=redis://...` or `--cache=bigtable://PROJECT/INSTANCE/TABLE` to store every geocoding result, including "no match" results, in a key/value store. Later runs read from the cache first and only call Smarty for addresses they have not seen.

### Refreshing stale cache entries

Smarty's coverage improves over time, so an address that failed to match last year may match today. With a BigTable cache, you can ask `geocode-csv` to re-geocode cached results as it reads them:

```sh
geocode-csv --spec address_spec.json \
  --cache=bigtable://PROJECT/INSTANCE/TABLE \
  --refresh-failures-after-days=90 \
  --refresh-failures-max-attempts=4 \
  --refresh-rate=0.1 \
  < in.csv > out.csv
```

- `--refresh-failures-after-days=N` re-checks a cached "no match" once it is `N` days old. Each failed re-check doubles the wait (`N`, `2N`, `4N`, ...).
- `--refresh-failures-max-attempts=M` stops re-checking a failure after `M` refreshes.
- `--refresh-successes-after-days=N` (optional) re-checks a successful geocode once it is `N` days old. If Smarty then returns no match, the old result is kept.
- `--refresh-rate=F`, with `F` in `(0, 1]`, is required whenever a refresh period is set. Of the cache entries this run reads that are due for refresh, only fraction `F` are actually re-geocoded on any given day. The decision is derived from the cache key and the date, so the same entry gets the same answer all day. Use this to stop a cache that was loaded all at once from coming due all at once.

Refresh only works with BigTable, because Redis does not record when a key was written. `--cache-hits-only` disables refresh.

Rows written by this version can still be read by older versions of `geocode-csv`.

## Build

You'll need to run:

```bash
git submodule update --init
```

...to pull in the C++ source for `libpostal`.

You will also need to [install `protoc`](https://grpc.io/docs/protoc-installation/):

```bash
# Linux.
sudo apt install protobuf-compiler

# Mac.
brew install protobuf
```

## A note about Macs

We provide pre-built Mac binaries for Intel- and M1-based Macs. These binaries use "ad-hoc" signatures, so you may need to [set appropriate security settings](https://support.apple.com/en-us/HT202491) or run:

```sh
xattr -d com.apple.quarantine geocode-csv
```

[smarty]: https://smarty.com/
