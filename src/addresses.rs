//! Types related to addresses.

use anyhow::{format_err, Context};
use csv::StringRecord;
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    fs::File,
    path::Path,
};

use crate::{geocoders::Geocoder, Result};

/// An address record that we can pass to a geocoder.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Address {
    /// Either the street, or the entire address as a string. This must always
    /// be present.
    pub street: String,
    /// The city, if any.
    pub city: Option<String>,
    /// The state, if any.
    pub state: Option<String>,
    /// The zipcode, if any.
    pub zipcode: Option<String>,
}

impl Address {
    /// A valid `Address` has a non-empty `street` field. (And whitespace
    /// doesn't count as non-empty.)
    pub fn is_valid(&self) -> bool {
        !self.street.trim().is_empty()
    }

    /// The `city` field, or an empty string.
    pub fn city_str(&self) -> &str {
        self.city.as_ref().map(|s| &s[..]).unwrap_or("")
    }

    /// The `state` field, or an empty string.
    pub fn state_str(&self) -> &str {
        self.state.as_ref().map(|s| &s[..]).unwrap_or("")
    }

    /// The `zipcode` field, or an empty string.
    pub fn zipcode_str(&self) -> &str {
        self.zipcode.as_ref().map(|s| &s[..]).unwrap_or("")
    }

    /// Is `self` equal to `other`, ignoring ASCII case?
    pub fn eq_ignore_ascii_case(&self, other: &Address) -> bool {
        self.street.eq_ignore_ascii_case(&other.street)
            && self.city_str().eq_ignore_ascii_case(other.city_str())
            && self.state_str().eq_ignore_ascii_case(other.state_str())
            && self.zipcode_str().eq_ignore_ascii_case(other.zipcode_str())
    }
}

#[test]
fn address_is_valid_does_not_allow_empty_streets() {
    let address_for = |street: &str| Address {
        street: street.to_owned(),
        city: None,
        state: None,
        zipcode: None,
    };
    assert!(!address_for("").is_valid());
    assert!(!address_for("   ").is_valid());
    assert!(!address_for(" \t\n  ").is_valid());
    assert!(address_for("123 Main Street").is_valid());
}

/// Either a column name, or a list of names.
///
/// `K` is typically either a `String` (for a column name) or a `usize` (for a
/// column index).
#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(untagged, deny_unknown_fields)]
pub enum ColumnKeyOrKeys<K: Eq> {
    /// The name of a single column.
    Key(K),
    /// The names of multiple columns, which should be joined using a space.
    Keys(Vec<K>),
}

impl ColumnKeyOrKeys<usize> {
    /// Given a CSV row, extract an `Address` value to send to our geocoder.
    pub fn extract_from_record<'a>(
        &self,
        record: &'a StringRecord,
    ) -> Result<Cow<'a, str>> {
        match self {
            ColumnKeyOrKeys::Key(key) => Ok(Cow::Borrowed(&record[*key])),
            ColumnKeyOrKeys::Keys(keys) => {
                // Allocate an empty string with some reserved space so we maybe don't
                // need to reallocate it every time we append.
                let mut extracted = String::with_capacity(40);
                for key in keys {
                    let s = &record[*key];
                    if extracted.is_empty() {
                        extracted.push_str(s);
                    } else if extracted.ends_with(s) {
                        // Already there, so ignore it. This appears in a lot of
                        // real-world databases, for some reason.
                    } else {
                        extracted.push(' ');
                        extracted.push_str(s);
                    }
                }
                Ok(Cow::Owned(extracted))
            }
        }
    }
}

#[test]
fn extract_collapses_duplicate_suffixes() {
    // This seems really arbitrary, but it consistently appears in many
    // real-world databases.
    //
    // I wonder if the equivalent "prefix" case is common?
    use std::iter::FromIterator;
    let record = StringRecord::from_iter(&["100", "Main Street #302", "#302"]);
    let keys = ColumnKeyOrKeys::Keys(vec![0, 1, 2]);
    assert_eq!(
        keys.extract_from_record(&record).unwrap(),
        "100 Main Street #302",
    );
}

/// The column names from a CSV file that we want to use as addresses.
///
/// `K` is typically either a `String` (for a column name) or a `usize` (for a
/// column index).
#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AddressColumnKeys<K: Default + Eq> {
    /// The name of street column or columns. May also be specified as
    /// "house_number_and_street" or "address".
    #[serde(alias = "house_number_and_street", alias = "address", alias = "glob")]
    pub street: ColumnKeyOrKeys<K>,
    /// The city column, if any.
    #[serde(default)]
    pub city: Option<K>,
    /// The state column, if any.
    #[serde(default)]
    pub state: Option<K>,
    /// The zipcode column, if any. May also be specified as
    /// "postcode".
    #[serde(default, alias = "postcode")]
    pub zipcode: Option<K>,
}

impl AddressColumnKeys<usize> {
    /// Given a CSV row, extract an `Address` value to send to our geocoder.
    pub fn extract_address_from_record(
        &self,
        record: &'_ StringRecord,
    ) -> Result<Address> {
        Ok(Address {
            street: self.street.extract_from_record(record)?.into_owned(),
            city: self.city.map(|c| record[c].to_owned()),
            state: self.state.map(|s| record[s].to_owned()),
            zipcode: self.zipcode.map(|z| record[z].to_owned()),
        })
    }
}

#[test]
fn extract_simple_address_from_record() {
    use std::iter::FromIterator;
    let record = StringRecord::from_iter(&[
        "1600 Pennsylvania Avenue NW, Washington DC, 20500",
    ]);
    let keys = AddressColumnKeys {
        street: ColumnKeyOrKeys::Key(0),
        city: None,
        state: None,
        zipcode: None,
    };
    assert_eq!(
        keys.extract_address_from_record(&record).unwrap(),
        Address {
            street: "1600 Pennsylvania Avenue NW, Washington DC, 20500".to_owned(),
            city: None,
            state: None,
            zipcode: None,
        },
    );
}

#[test]
fn extract_complex_address_from_record() {
    use std::iter::FromIterator;
    let record = StringRecord::from_iter(&[
        "1600",
        "Pennsylvania Avenue NW",
        "Washington",
        "DC",
        "20500",
    ]);
    let keys = AddressColumnKeys {
        street: ColumnKeyOrKeys::Keys(vec![0, 1]),
        city: Some(2),
        state: Some(3),
        zipcode: Some(4),
    };
    assert_eq!(
        keys.extract_address_from_record(&record).unwrap(),
        Address {
            street: "1600 Pennsylvania Avenue NW".to_owned(),
            city: Some("Washington".to_owned()),
            state: Some("DC".to_owned()),
            zipcode: Some("20500".to_owned()),
        },
    );
}

/// Return a prefixed column name of the form `"{prefix}_{column}`".
pub fn prefix_column_name(prefix: &str, column: &str) -> String {
    format!("{}_{}", prefix, column)
}

/// A map from column prefixes (e.g. "home", "work") to address column keys.
///
/// `K` is typically either a `String` (for a column name) or a `usize` (for a
/// column index).
#[derive(Debug, Deserialize, Eq, PartialEq)]
pub struct AddressColumnSpec<Key: Default + Eq> {
    /// A map from output column prefixes to address column keys.
    #[serde(flatten)]
    address_columns_by_prefix: HashMap<String, AddressColumnKeys<Key>>,
}

impl<Key: Default + Eq> AddressColumnSpec<Key> {
    /// The number of prefixes we want to include in our output.
    pub fn prefix_count(&self) -> usize {
        self.address_columns_by_prefix.len()
    }

    /// The address prefixes we want to include in our output.
    ///
    /// This **MUST** return the prefixes in the same order every time or our
    /// output will be corrupted.
    pub fn prefixes(&self) -> Vec<&str> {
        let mut prefixes = self
            .address_columns_by_prefix
            .keys()
            .map(|k| &k[..])
            .collect::<Vec<_>>();
        // Do not remove this `sort`! This can be unstable because strings give
        // the same result with stable and unstable sorts.
        prefixes.sort_unstable();
        prefixes
    }

    /// Look up an `AddressColumnKeys` by prefix.
    pub fn get(&self, prefix: &str) -> Option<&AddressColumnKeys<Key>> {
        self.address_columns_by_prefix.get(prefix)
    }

    /// What column should we remove from the input records in order
    /// to prevent duplicate columns?
    ///
    /// Returns the name and index of each column to remove, in order.
    pub fn duplicate_columns<'header>(
        &self,
        geocoder: &dyn Geocoder,
        header: &'header StringRecord,
    ) -> Result<Vec<(&'header str, usize)>> {
        // Get all our column names for all prefixes, and insert them into a
        // hash table.
        let mut output_column_names = HashSet::new();
        for prefix in self.prefixes() {
            for name in geocoder.column_names() {
                let full_name = prefix_column_name(prefix, name);
                if !output_column_names.insert(full_name.clone()) {
                    return Err(format_err!("duplicate column name {:?}", full_name));
                }
            }
        }

        // Decide which columns of `header` need to be removed.
        let mut duplicate_columns = vec![];
        for (i, col) in header.iter().enumerate() {
            if output_column_names.contains(col) {
                duplicate_columns.push((col, i));
            }
        }
        Ok(duplicate_columns)
    }
}

#[test]
#[ignore]
fn find_columns_to_remove() {
    use std::iter::FromIterator;

    use crate::geocoders::{shared_http_client, smarty::Smarty, MatchStrategy};

    let address_column_spec_json = r#"{
        "home": {
            "house_number_and_street": ["home_number", "home_street"],
            "city": "home_city",
            "state": "home_state",
            "postcode": "home_zip"
        },
        "work": {
            "address": "work_address"
        }
    }"#;
    let spec: AddressColumnSpec<String> =
        serde_json::from_str(address_column_spec_json).unwrap();

    let geocoder = Smarty::new(
        MatchStrategy::Strict,
        "us-standard-cloud".to_owned(),
        None,
        shared_http_client(1),
    )
    .unwrap();
    let header =
        StringRecord::from_iter(&["existing", "home_addressee", "work_addressee"]);
    let indices = spec.duplicate_columns(&geocoder, &header).unwrap();
    assert_eq!(indices, vec![("home_addressee", 1), ("work_addressee", 2)]);
}

impl AddressColumnSpec<String> {
    /// Load an `AddressColumnSpec` from a file.
    pub fn from_path(path: &Path) -> Result<Self> {
        let f = File::open(path)
            .with_context(|| format_err!("cannot open {}", path.display()))?;
        serde_json::from_reader(f)
            .with_context(|| format_err!("error parsing {}", path.display()))
    }

    /// Given an `AddressColumnSpec` using strings, and the header row of a CSV
    /// file, convert it into a `AddressColumnSpec<usize>` containing the column
    /// indices.
    pub fn convert_to_indices_using_headers(
        &self,
        headers: &StringRecord,
    ) -> Result<AddressColumnSpec<usize>> {
        let mut header_columns = HashMap::new();
        for (idx, header) in headers.iter().enumerate() {
            if let Some(_existing) = header_columns.insert(header, idx) {
                return Err(format_err!("duplicate header column `{}`", header));
            }
        }
        self.convert_to_indices(&header_columns)
    }
}

#[test]
fn convert_address_column_spec_to_indices() {
    use std::iter::FromIterator;
    let headers = StringRecord::from_iter(&[
        "home_number",
        "home_street",
        "home_city",
        "home_state",
        "home_zip",
        "work_address",
    ]);
    let address_column_spec_json = r#"{
   "home": {
       "house_number_and_street": ["home_number", "home_street"],
       "city": "home_city",
       "state": "home_state",
       "postcode": "home_zip"
   },
   "work": {
       "address": "work_address"
   }
}"#;
    let address_column_spec: AddressColumnSpec<String> =
        serde_json::from_str(address_column_spec_json).unwrap();

    let mut expected = HashMap::new();
    expected.insert(
        "home".to_owned(),
        AddressColumnKeys {
            street: ColumnKeyOrKeys::Keys(vec![0, 1]),
            city: Some(2),
            state: Some(3),
            zipcode: Some(4),
        },
    );
    expected.insert(
        "work".to_owned(),
        AddressColumnKeys {
            street: ColumnKeyOrKeys::Key(5),
            city: None,
            state: None,
            zipcode: None,
        },
    );
    assert_eq!(
        address_column_spec
            .convert_to_indices_using_headers(&headers)
            .unwrap(),
        AddressColumnSpec::<usize> {
            address_columns_by_prefix: expected,
        },
    );
}

/// A value which can be converted from using string indices to numeric indices.
trait ConvertToIndices {
    type Output;

    /// Convert this value from using string indices to numeric indices.
    fn convert_to_indices(
        &self,
        header_columns: &HashMap<&str, usize>,
    ) -> Result<Self::Output>;
}

impl ConvertToIndices for String {
    type Output = usize;

    fn convert_to_indices(
        &self,
        header_columns: &HashMap<&str, usize>,
    ) -> Result<Self::Output> {
        header_columns
            .get(&self[..])
            .copied()
            .ok_or_else(|| format_err!("could not find column `{}` in header", self))
    }
}

impl ConvertToIndices for ColumnKeyOrKeys<String> {
    type Output = ColumnKeyOrKeys<usize>;

    fn convert_to_indices(
        &self,
        header_columns: &HashMap<&str, usize>,
    ) -> Result<Self::Output> {
        match self {
            ColumnKeyOrKeys::Key(key) => Ok(ColumnKeyOrKeys::Key(
                key.convert_to_indices(header_columns)?,
            )),
            ColumnKeyOrKeys::Keys(keys) => Ok(ColumnKeyOrKeys::Keys(
                keys.iter()
                    .map(|k| k.convert_to_indices(header_columns))
                    .collect::<Result<Vec<_>>>()?,
            )),
        }
    }
}

impl ConvertToIndices for AddressColumnKeys<String> {
    type Output = AddressColumnKeys<usize>;

    fn convert_to_indices(
        &self,
        header_columns: &HashMap<&str, usize>,
    ) -> Result<Self::Output> {
        Ok(AddressColumnKeys {
            street: self.street.convert_to_indices(header_columns)?,
            city: self
                .city
                .as_ref()
                .map(|c| c.convert_to_indices(header_columns))
                .transpose()?,
            state: self
                .state
                .as_ref()
                .map(|s| s.convert_to_indices(header_columns))
                .transpose()?,
            zipcode: self
                .zipcode
                .as_ref()
                .map(|z| z.convert_to_indices(header_columns))
                .transpose()?,
        })
    }
}

impl ConvertToIndices for AddressColumnSpec<String> {
    type Output = AddressColumnSpec<usize>;

    fn convert_to_indices(
        &self,
        header_columns: &HashMap<&str, usize>,
    ) -> Result<Self::Output> {
        let mut address_columns_by_prefix = HashMap::new();
        for (prefix, address_columns) in &self.address_columns_by_prefix {
            address_columns_by_prefix.insert(
                prefix.to_owned(),
                address_columns.convert_to_indices(header_columns)?,
            );
        }
        Ok(AddressColumnSpec {
            address_columns_by_prefix,
        })
    }
}
