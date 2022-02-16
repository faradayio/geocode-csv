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

## A note about Macs

We provide pre-built Mac binaries for Intel- and M1-based Macs. These binaries use "ad-hoc" signatures, so you may need to [set appropriate security settings](https://support.apple.com/en-us/HT202491) or run:

```sh
xattr -d com.apple.quarantine geocode-csv
```

[smarty]: https://smarty.com/
